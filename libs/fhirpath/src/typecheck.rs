//! Type checking and resolution pass
//!
//! This pass takes structural HIR (produced by `Analyzer`) and fully annotates it with:
//! - element type sets (including choice types / unions)
//! - collection cardinalities
//!
//! Type information is resolved using:
//! - `FhirContext` for navigation (StructureDefinitions)
//! - function metadata + built-in FHIRPath typing rules

use crate::error::{Error, Result};
use crate::functions::FunctionRegistry;
use crate::hir::{HirBinaryOperator, HirNode, HirUnaryOperator, PathSegmentHir};
use crate::types::{
    Cardinality, ExprType, NamedType, TypeId, TypeNamespace, TypeRegistry, TypeSet,
};
use crate::value::{Value, ValueData};
use std::sync::Arc;
use zunder_context::FhirContext;

#[derive(Clone, Debug)]
struct TypeContext {
    this: ExprType,
    strict: bool,
}

impl TypeContext {
    fn with_this(&self, this: ExprType) -> Self {
        Self {
            this,
            strict: self.strict,
        }
    }
}

/// Type checking and resolution pass.
pub struct TypePass {
    type_registry: Arc<TypeRegistry>,
    function_registry: Arc<FunctionRegistry>,
    fhir_context: Arc<dyn FhirContext>,
}

impl TypePass {
    pub fn new(
        type_registry: Arc<TypeRegistry>,
        function_registry: Arc<FunctionRegistry>,
        fhir_context: Arc<dyn FhirContext>,
    ) -> Self {
        Self {
            type_registry,
            function_registry,
            fhir_context,
        }
    }

    /// Resolve types in a HIR node.
    ///
    /// * `context_type`: Optional root type name (e.g., "Patient") used for
    ///   semantic type annotation and (optionally) StructureDefinition navigation.
    /// * `strict`: If `true`, unknown fields on resolvable FHIR types become errors.
    pub fn resolve(
        &self,
        node: HirNode,
        context_type: Option<String>,
        strict: bool,
    ) -> Result<HirNode> {
        let this = context_type
            .as_deref()
            .map(|name| self.this_type_from_name(name))
            .unwrap_or_else(|| ExprType::unknown().with_cardinality(Cardinality::ZERO_TO_ONE));

        let ctx = TypeContext { this, strict };
        self.visit_node(node, &ctx)
    }

    fn is_fhir_type_name(&self, type_name: &str) -> bool {
        matches!(
            self.fhir_context
                .get_core_structure_definition_by_type(type_name),
            Ok(Some(_))
        )
    }

    /// Check whether `actual_type` is the same as, or a subtype of, `target_type`.
    ///
    /// Uses StructureDefinition.baseDefinition chain (core model only).
    fn fhir_type_is_a(&self, actual_type: &str, target_type: &str) -> bool {
        let target_lower = target_type.to_ascii_lowercase();
        let mut current = actual_type.to_string();
        loop {
            if current.to_ascii_lowercase() == target_lower {
                return true;
            }

            let sd = match self
                .fhir_context
                .get_core_structure_definition_by_type(&current)
            {
                Ok(Some(sd)) => sd,
                _ => return false,
            };

            let Some(base_def) = sd.base_definition.as_deref() else {
                return false;
            };

            let Some(base_type) = base_def.strip_prefix("http://hl7.org/fhir/StructureDefinition/")
            else {
                return false;
            };

            current = base_type.to_string();
        }
    }

    fn should_skip_root_type_prefix(&self, base_types: &TypeSet, segment: &str) -> bool {
        if base_types.is_unknown() {
            return false;
        }
        if !self.is_fhir_type_name(segment) {
            return false;
        }

        base_types.iter().any(|t| {
            t.namespace == TypeNamespace::Fhir && self.fhir_type_is_a(t.name.as_ref(), segment)
        })
    }

    fn this_type_from_name(&self, name: &str) -> ExprType {
        if let Some(id) = self.type_registry.get_type_id_by_name(name) {
            return self
                .type_registry
                .expr_from_system_type(id, Cardinality::ZERO_TO_ONE);
        }

        ExprType {
            types: TypeSet::singleton(self.type_registry.fhir_named(name)),
            cardinality: Cardinality::ZERO_TO_ONE,
        }
    }

    fn visit_node(&self, node: HirNode, ctx: &TypeContext) -> Result<HirNode> {
        match node {
            HirNode::Literal { value, .. } => Ok(HirNode::Literal {
                ty: self.infer_literal_type(&value),
                value,
            }),

            HirNode::Variable { var_id, name, .. } => {
                let ty = self.resolve_variable_type(var_id, name.as_deref(), ctx);
                Ok(HirNode::Variable { var_id, name, ty })
            }

            HirNode::Path { base, segments, .. } => {
                let typed_base = self.visit_node(*base, ctx)?;
                let base_ty = typed_base.result_type().unwrap_or_else(ExprType::unknown);
                let result_ty = self.infer_path_type(&base_ty, &segments, ctx.strict)?;

                Ok(HirNode::Path {
                    base: Box::new(typed_base),
                    segments,
                    result_ty,
                })
            }

            HirNode::BinaryOp {
                op,
                left,
                right,
                impl_id,
                ..
            } => {
                let typed_left = self.visit_node(*left, ctx)?;
                let typed_right = self.visit_node(*right, ctx)?;
                let left_ty = typed_left.result_type().unwrap_or_else(ExprType::unknown);
                let right_ty = typed_right.result_type().unwrap_or_else(ExprType::unknown);
                let result_ty = self.infer_binary_result_type(op, &left_ty, &right_ty);

                Ok(HirNode::BinaryOp {
                    op,
                    left: Box::new(typed_left),
                    right: Box::new(typed_right),
                    impl_id,
                    result_ty,
                })
            }

            HirNode::UnaryOp { op, expr, .. } => {
                let typed_expr = self.visit_node(*expr, ctx)?;
                let expr_ty = typed_expr.result_type().unwrap_or_else(ExprType::unknown);
                let result_ty = self.infer_unary_result_type(op, &expr_ty);
                Ok(HirNode::UnaryOp {
                    op,
                    expr: Box::new(typed_expr),
                    result_ty,
                })
            }

            HirNode::FunctionCall { func_id, args, .. } => {
                let typed_args: Result<Vec<HirNode>> =
                    args.into_iter().map(|a| self.visit_node(a, ctx)).collect();
                let typed_args = typed_args?;
                let result_ty = self.infer_function_call_type(func_id, None, &typed_args);

                Ok(HirNode::FunctionCall {
                    func_id,
                    args: typed_args,
                    result_ty,
                })
            }

            HirNode::MethodCall {
                base,
                func_id,
                args,
                ..
            } => {
                let typed_base = self.visit_node(*base, ctx)?;
                let typed_args: Result<Vec<HirNode>> =
                    args.into_iter().map(|a| self.visit_node(a, ctx)).collect();
                let typed_args = typed_args?;
                let base_ty = typed_base.result_type().unwrap_or_else(ExprType::unknown);
                let result_ty = self.infer_function_call_type(func_id, Some(&base_ty), &typed_args);

                Ok(HirNode::MethodCall {
                    base: Box::new(typed_base),
                    func_id,
                    args: typed_args,
                    result_ty,
                })
            }

            HirNode::Where {
                collection,
                predicate_hir,
                predicate_plan_id,
                ..
            } => {
                let typed_collection = self.visit_node(*collection, ctx)?;
                let coll_ty = typed_collection
                    .result_type()
                    .unwrap_or_else(ExprType::unknown);
                let predicate_ctx = ctx.with_this(ExprType {
                    types: coll_ty.types.clone(),
                    cardinality: Cardinality::ZERO_TO_ONE,
                });
                let typed_predicate = self.visit_node(*predicate_hir, &predicate_ctx)?;

                Ok(HirNode::Where {
                    collection: Box::new(typed_collection),
                    predicate_hir: Box::new(typed_predicate),
                    predicate_plan_id,
                    result_ty: ExprType {
                        types: coll_ty.types,
                        cardinality: Cardinality {
                            min: 0,
                            max: coll_ty.cardinality.max,
                        },
                    },
                })
            }

            HirNode::Select {
                collection,
                projection_hir,
                projection_plan_id,
                ..
            } => {
                let typed_collection = self.visit_node(*collection, ctx)?;
                let coll_ty = typed_collection
                    .result_type()
                    .unwrap_or_else(ExprType::unknown);
                let projection_ctx = ctx.with_this(ExprType {
                    types: coll_ty.types.clone(),
                    cardinality: Cardinality::ZERO_TO_ONE,
                });
                let typed_projection = self.visit_node(*projection_hir, &projection_ctx)?;
                let proj_ty = typed_projection
                    .result_type()
                    .unwrap_or_else(ExprType::unknown);

                Ok(HirNode::Select {
                    collection: Box::new(typed_collection),
                    projection_hir: Box::new(typed_projection),
                    projection_plan_id,
                    result_ty: ExprType {
                        types: proj_ty.types,
                        cardinality: coll_ty.cardinality.multiply(proj_ty.cardinality),
                    },
                })
            }

            HirNode::Repeat {
                collection,
                projection_hir,
                projection_plan_id,
                ..
            } => {
                let typed_collection = self.visit_node(*collection, ctx)?;
                let coll_ty = typed_collection
                    .result_type()
                    .unwrap_or_else(ExprType::unknown);
                let projection_ctx = ctx.with_this(ExprType {
                    types: coll_ty.types.clone(),
                    cardinality: Cardinality::ZERO_TO_ONE,
                });
                let typed_projection = self.visit_node(*projection_hir, &projection_ctx)?;
                let proj_ty = typed_projection
                    .result_type()
                    .unwrap_or_else(ExprType::unknown);

                Ok(HirNode::Repeat {
                    collection: Box::new(typed_collection),
                    projection_hir: Box::new(typed_projection),
                    projection_plan_id,
                    result_ty: ExprType {
                        types: proj_ty.types,
                        cardinality: Cardinality::ZERO_TO_MANY,
                    },
                })
            }

            HirNode::Aggregate {
                collection,
                aggregator_hir,
                init_value_hir,
                aggregator_plan_id,
                ..
            } => {
                let typed_collection = self.visit_node(*collection, ctx)?;
                let coll_ty = typed_collection
                    .result_type()
                    .unwrap_or_else(ExprType::unknown);

                let agg_ctx = ctx.with_this(ExprType {
                    types: coll_ty.types.clone(),
                    cardinality: Cardinality::ZERO_TO_ONE,
                });
                let typed_aggregator = self.visit_node(*aggregator_hir, &agg_ctx)?;
                let agg_ty = typed_aggregator
                    .result_type()
                    .unwrap_or_else(ExprType::unknown);

                let typed_init = match init_value_hir {
                    Some(init) => Some(Box::new(self.visit_node(*init, ctx)?)),
                    None => None,
                };

                Ok(HirNode::Aggregate {
                    collection: Box::new(typed_collection),
                    aggregator_hir: Box::new(typed_aggregator),
                    init_value_hir: typed_init,
                    aggregator_plan_id,
                    result_ty: ExprType {
                        types: agg_ty.types,
                        cardinality: Cardinality::ZERO_TO_ONE,
                    },
                })
            }

            HirNode::Exists {
                collection,
                predicate_hir,
                predicate_plan_id,
                ..
            } => {
                let typed_collection = self.visit_node(*collection, ctx)?;
                let coll_ty = typed_collection
                    .result_type()
                    .unwrap_or_else(ExprType::unknown);
                let predicate_ctx = ctx.with_this(ExprType {
                    types: coll_ty.types,
                    cardinality: Cardinality::ZERO_TO_ONE,
                });
                let typed_predicate = match predicate_hir {
                    Some(pred) => Some(Box::new(self.visit_node(*pred, &predicate_ctx)?)),
                    None => None,
                };

                Ok(HirNode::Exists {
                    collection: Box::new(typed_collection),
                    predicate_hir: typed_predicate,
                    predicate_plan_id,
                    result_ty: self
                        .type_registry
                        .expr_from_system_type(TypeId::Boolean, Cardinality::ONE_TO_ONE),
                })
            }

            HirNode::All {
                collection,
                predicate_hir,
                predicate_plan_id,
                ..
            } => {
                let typed_collection = self.visit_node(*collection, ctx)?;
                let coll_ty = typed_collection
                    .result_type()
                    .unwrap_or_else(ExprType::unknown);
                let predicate_ctx = ctx.with_this(ExprType {
                    types: coll_ty.types,
                    cardinality: Cardinality::ZERO_TO_ONE,
                });
                let typed_predicate = self.visit_node(*predicate_hir, &predicate_ctx)?;

                Ok(HirNode::All {
                    collection: Box::new(typed_collection),
                    predicate_hir: Box::new(typed_predicate),
                    predicate_plan_id,
                    result_ty: self
                        .type_registry
                        .expr_from_system_type(TypeId::Boolean, Cardinality::ONE_TO_ONE),
                })
            }

            HirNode::TypeOp {
                op,
                expr,
                type_specifier,
                ..
            } => {
                let typed_expr = self.visit_node(*expr, ctx)?;
                let expr_ty = typed_expr.result_type().unwrap_or_else(ExprType::unknown);
                let spec_ty = self.parse_type_specifier(&type_specifier);
                let result_ty = match op {
                    crate::hir::HirTypeOperator::Is => self
                        .type_registry
                        .expr_from_system_type(TypeId::Boolean, Cardinality::ONE_TO_ONE),
                    crate::hir::HirTypeOperator::As => ExprType {
                        types: spec_ty,
                        cardinality: Cardinality {
                            min: 0,
                            max: expr_ty.cardinality.max,
                        },
                    },
                };

                Ok(HirNode::TypeOp {
                    op,
                    expr: Box::new(typed_expr),
                    type_specifier,
                    result_ty,
                })
            }
        }
    }

    fn resolve_variable_type(
        &self,
        var_id: crate::hir::VariableId,
        name: Option<&str>,
        ctx: &TypeContext,
    ) -> ExprType {
        match var_id {
            0 => ctx.this.clone(),
            1 | 2 => self
                .type_registry
                .expr_from_system_type(TypeId::Integer, Cardinality::ZERO_TO_ONE),
            _ => {
                // Heuristic: common runtime-provided roots.
                if let Some(n) = name {
                    if n == "resource" || n == "context" || n == "root" || n == "rootResource" {
                        return ctx.this.clone();
                    }
                    if n == "profile" {
                        return self
                            .type_registry
                            .expr_from_system_type(TypeId::String, Cardinality::ZERO_TO_ONE);
                    }
                }
                ExprType::unknown().with_cardinality(Cardinality::ZERO_TO_ONE)
            }
        }
    }

    fn infer_literal_type(&self, value: &Value) -> ExprType {
        match value.data() {
            ValueData::Empty => ExprType::empty(),
            ValueData::Boolean(_) => self
                .type_registry
                .expr_from_system_type(TypeId::Boolean, Cardinality::ONE_TO_ONE),
            ValueData::Integer(_) => self
                .type_registry
                .expr_from_system_type(TypeId::Integer, Cardinality::ONE_TO_ONE),
            ValueData::Decimal(_) => self
                .type_registry
                .expr_from_system_type(TypeId::Decimal, Cardinality::ONE_TO_ONE),
            ValueData::String(_) => self
                .type_registry
                .expr_from_system_type(TypeId::String, Cardinality::ONE_TO_ONE),
            ValueData::Date { .. } => self
                .type_registry
                .expr_from_system_type(TypeId::Date, Cardinality::ONE_TO_ONE),
            ValueData::DateTime { .. } => self
                .type_registry
                .expr_from_system_type(TypeId::DateTime, Cardinality::ONE_TO_ONE),
            ValueData::Time { .. } => self
                .type_registry
                .expr_from_system_type(TypeId::Time, Cardinality::ONE_TO_ONE),
            ValueData::Quantity { .. } => self
                .type_registry
                .expr_from_system_type(TypeId::Quantity, Cardinality::ONE_TO_ONE),
            ValueData::Object(_) => ExprType {
                types: TypeSet::unknown(),
                cardinality: Cardinality::ONE_TO_ONE,
            },
            ValueData::LazyJson { .. } => {
                // Materialize lazy JSON and recursively infer type
                let materialized = value.materialize();
                self.infer_literal_type(&materialized)
            }
        }
    }

    fn infer_path_type(
        &self,
        base: &ExprType,
        segments: &[PathSegmentHir],
        strict: bool,
    ) -> Result<ExprType> {
        let mut current = base.clone();

        for (idx, seg) in segments.iter().enumerate() {
            if idx == 0 {
                if let PathSegmentHir::Field(field) = seg {
                    // Per spec: a path may start with the type of the context (or a supertype).
                    // This is a no-op navigation (it yields the input collection).
                    if self.should_skip_root_type_prefix(&current.types, field) {
                        continue;
                    }
                }
            }
            match seg {
                PathSegmentHir::Field(field) => {
                    let (types, elem_card) =
                        self.resolve_field(current.types.clone(), field, strict)?;
                    current = ExprType {
                        types,
                        cardinality: current.cardinality.multiply(elem_card),
                    };
                }
                PathSegmentHir::Index(_) => {
                    current = ExprType {
                        types: current.types,
                        cardinality: current.cardinality.at_most_one(),
                    };
                }
                PathSegmentHir::Choice(choice) => {
                    // Analyzer currently never emits this; treat as field navigation for compatibility.
                    let (types, elem_card) =
                        self.resolve_field(current.types.clone(), choice, strict)?;
                    current = ExprType {
                        types,
                        cardinality: current.cardinality.multiply(elem_card),
                    };
                }
            }
        }

        Ok(current)
    }

    fn resolve_field(
        &self,
        base_types: TypeSet,
        field: &str,
        strict: bool,
    ) -> Result<(TypeSet, Cardinality)> {
        if base_types.is_unknown() {
            // When base is unknown, check if field is a FHIR type name
            // This helps with type inference: Observation.value will know that after
            // navigating to "Observation" (which might fail at runtime if type doesn't match),
            // we're working with an Observation type
            if self.is_fhir_type_name(field) {
                return Ok((
                    TypeSet::singleton(self.type_registry.fhir_named(field)),
                    Cardinality::ZERO_TO_ONE,
                ));
            }
            return Ok((TypeSet::unknown(), Cardinality::ZERO_TO_MANY));
        }

        let mut resolved_types: Vec<NamedType> = Vec::new();
        let mut resolved_any = false;
        let mut saw_resolvable_base = false;
        let mut max_elem_card: Option<u32> = Some(0);

        for base in base_types.iter() {
            if base.namespace != TypeNamespace::Fhir {
                continue;
            }

            // If the base type isn't known to the context, don't enforce strictness.
            let sd_exists = matches!(
                self.fhir_context
                    .get_core_structure_definition_by_type(base.name.as_ref()),
                Ok(Some(_))
            );
            if !sd_exists {
                continue;
            }
            saw_resolvable_base = true;

            let elem = match self
                .fhir_context
                .get_element_type(base.name.as_ref(), field)
            {
                Ok(v) => v,
                Err(_) => {
                    continue;
                }
            };

            let Some(elem) = elem else {
                continue;
            };

            resolved_any = true;

            let card = Cardinality {
                min: elem.min,
                max: if elem.is_array { elem.max } else { Some(1) },
            };

            max_elem_card = match (max_elem_card, card.max) {
                (Some(a), Some(b)) => Some(a.max(b)),
                _ => None,
            };

            for code in &elem.type_codes {
                if let Some(named) = self.named_type_from_code(code) {
                    resolved_types.push(named);
                }
            }
        }

        if !resolved_any {
            if strict && saw_resolvable_base {
                // In strict mode, unknown fields on known FHIR types are errors.
                // (If the base type itself wasn't resolvable, we intentionally didn't error.)
                return Err(Error::TypeError(format!(
                    "Path segment '{}' does not exist on the current type",
                    field
                )));
            }
            return Ok((TypeSet::unknown(), Cardinality::ZERO_TO_MANY));
        }

        let types = TypeSet::from_many(resolved_types);
        Ok((
            types,
            Cardinality {
                min: 0,
                max: max_elem_card,
            },
        ))
    }

    fn named_type_from_code(&self, code: &str) -> Option<NamedType> {
        // Common canonical prefixes
        let code = code
            .strip_prefix("http://hl7.org/fhirpath/System.")
            .unwrap_or(code);
        let code = code
            .strip_prefix("http://hl7.org/fhir/StructureDefinition/")
            .unwrap_or(code);

        if let Some(id) = self.type_registry.get_type_id_by_name(code) {
            return Some(self.type_registry.system_named(id));
        }

        // Lowercase alias mapping for FHIR primitives (string, boolean, ...)
        if let Some(id) = self
            .type_registry
            .get_type_id_by_name(code.to_ascii_lowercase().as_str())
        {
            return Some(self.type_registry.system_named(id));
        }

        Some(self.type_registry.fhir_named(code))
    }

    fn parse_type_specifier(&self, spec: &str) -> TypeSet {
        let spec = spec.trim();
        if let Some(rest) = spec.strip_prefix("System.") {
            if let Some(id) = self.type_registry.get_type_id_by_name(rest) {
                return self.type_registry.system_set(id);
            }
            return TypeSet::unknown();
        }
        if let Some(rest) = spec.strip_prefix("FHIR.") {
            return TypeSet::singleton(self.type_registry.fhir_named(rest));
        }

        if let Some(id) = self.type_registry.get_type_id_by_name(spec) {
            return self.type_registry.system_set(id);
        }
        TypeSet::singleton(self.type_registry.fhir_named(spec))
    }

    fn infer_unary_result_type(&self, op: HirUnaryOperator, expr: &ExprType) -> ExprType {
        match op {
            HirUnaryOperator::Plus | HirUnaryOperator::Minus => ExprType {
                types: expr.types.clone(),
                cardinality: Cardinality::ZERO_TO_ONE,
            },
        }
    }

    fn infer_binary_result_type(
        &self,
        op: HirBinaryOperator,
        left: &ExprType,
        right: &ExprType,
    ) -> ExprType {
        use HirBinaryOperator::*;

        match op {
            Eq | Ne | Equivalent | NotEquivalent | Lt | Le | Gt | Ge | And | Or | Xor | Implies
            | In | Contains => self
                .type_registry
                .expr_from_system_type(TypeId::Boolean, Cardinality::ONE_TO_ONE),

            Add | Sub | Mul | Div | DivInt | Mod => ExprType {
                types: self.numeric_result_types(&left.types, &right.types),
                cardinality: Cardinality::ZERO_TO_ONE,
            },

            Concat => self
                .type_registry
                .expr_from_system_type(TypeId::String, Cardinality::ZERO_TO_ONE),

            Union => ExprType {
                types: left.types.union(&right.types),
                cardinality: left.cardinality.add_upper_bounds(right.cardinality),
            },
        }
    }

    fn numeric_result_types(&self, left: &TypeSet, right: &TypeSet) -> TypeSet {
        if left.is_unknown() || right.is_unknown() {
            return TypeSet::unknown();
        }

        let is_int_only = |ts: &TypeSet| {
            ts.iter()
                .all(|t| t.namespace == TypeNamespace::System && t.name.as_ref() == "Integer")
        };
        let is_decimal_only = |ts: &TypeSet| {
            ts.iter()
                .all(|t| t.namespace == TypeNamespace::System && t.name.as_ref() == "Decimal")
        };

        if is_int_only(left) && is_int_only(right) {
            return self.type_registry.system_set(TypeId::Integer);
        }
        if (is_decimal_only(left) || is_int_only(left))
            && (is_decimal_only(right) || is_int_only(right))
        {
            return self.type_registry.system_set(TypeId::Decimal);
        }
        TypeSet::unknown()
    }

    fn infer_function_call_type(
        &self,
        func_id: crate::hir::FunctionId,
        base: Option<&ExprType>,
        args: &[HirNode],
    ) -> ExprType {
        // Prefer metadata when it is concrete.
        if let Some(meta) = self.function_registry.get_metadata(func_id) {
            if meta.return_type != TypeId::Unknown {
                return self
                    .type_registry
                    .expr_from_system_type(meta.return_type, Cardinality::ZERO_TO_ONE);
            }
        }

        match func_id {
            // exists()
            11 => self
                .type_registry
                .expr_from_system_type(TypeId::Boolean, Cardinality::ONE_TO_ONE),
            // count()
            19 => self
                .type_registry
                .expr_from_system_type(TypeId::Integer, Cardinality::ONE_TO_ONE),

            // first(), last(), single()
            40..=42 => base
                .cloned()
                .unwrap_or_else(ExprType::unknown)
                .with_cardinality(Cardinality::ZERO_TO_ONE),

            // toString()
            100 => self
                .type_registry
                .expr_from_system_type(TypeId::String, Cardinality::ZERO_TO_ONE),
            // toInteger()
            303 => self
                .type_registry
                .expr_from_system_type(TypeId::Integer, Cardinality::ZERO_TO_ONE),
            // toDecimal()
            305 => self
                .type_registry
                .expr_from_system_type(TypeId::Decimal, Cardinality::ZERO_TO_ONE),
            // toBoolean()
            301 => self
                .type_registry
                .expr_from_system_type(TypeId::Boolean, Cardinality::ZERO_TO_ONE),
            // toDate()
            308 => self
                .type_registry
                .expr_from_system_type(TypeId::Date, Cardinality::ZERO_TO_ONE),
            // toDateTime()
            310 => self
                .type_registry
                .expr_from_system_type(TypeId::DateTime, Cardinality::ZERO_TO_ONE),
            // toTime()
            312 => self
                .type_registry
                .expr_from_system_type(TypeId::Time, Cardinality::ZERO_TO_ONE),

            // ofType(type)
            33 => {
                // ofType() narrows the collection to elements of the specified type
                // Returns 0..max(base) with the specified type
                if let Some(HirNode::Literal { value, .. }) = args.first() {
                    if let crate::value::ValueData::String(type_name) = value.data() {
                        let spec_ty = self.parse_type_specifier(type_name.as_ref());
                        let max = base.as_ref().map(|b| b.cardinality.max).unwrap_or(None);
                        return ExprType {
                            types: spec_ty,
                            cardinality: Cardinality { min: 0, max },
                        };
                    }
                }
                // Fallback if we can't determine the type
                ExprType::unknown()
            }

            // iif(condition, then, else?)
            300 => {
                let then_ty = args
                    .get(1)
                    .and_then(|n| n.result_type())
                    .unwrap_or_else(ExprType::unknown);
                let else_ty = args
                    .get(2)
                    .and_then(|n| n.result_type())
                    .unwrap_or_else(ExprType::empty);
                ExprType {
                    types: then_ty.types.union(&else_ty.types),
                    cardinality: Cardinality {
                        min: 0,
                        max: match (then_ty.cardinality.max, else_ty.cardinality.max) {
                            (Some(a), Some(b)) => Some(a.max(b)),
                            _ => None,
                        },
                    },
                }
            }

            _ => ExprType::unknown(),
        }
    }
}
