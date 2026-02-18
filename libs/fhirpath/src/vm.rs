//! Virtual Machine for executing compiled FHIRPath expressions
//!
//! The VM executes bytecode plans generated from HIR.

mod functions;
mod operations;

use crate::context::Context;
use crate::error::{Error, Result};
use crate::hir::HirBinaryOperator;
use crate::value::{Collection, Value, ValueData};
use functions::{aggregate_with_subplans, execute_function};
use operations::execute_binary_op;
use serde_json::Value as JsonValue;
use std::sync::Arc;

/// Unary plus operation
fn unary_plus(collection: Collection) -> Result<Collection> {
    if collection.is_empty() {
        return Ok(Collection::empty());
    }

    if collection.len() != 1 {
        return Err(Error::TypeError(
            "Unary plus requires singleton collection".into(),
        ));
    }

    let item = collection.iter().next().unwrap();
    match item.data() {
        ValueData::Integer(_) | ValueData::Decimal(_) => {
            // Return unchanged for numeric types
            Ok(collection)
        }
        _ => Err(Error::TypeError("Unary plus requires numeric type".into())),
    }
}

/// Unary minus operation
fn unary_minus(collection: Collection) -> Result<Collection> {
    if collection.is_empty() {
        return Ok(Collection::empty());
    }

    if collection.len() != 1 {
        return Err(Error::TypeError(
            "Unary minus requires singleton collection".into(),
        ));
    }

    let item = collection.iter().next().unwrap();
    match item.data() {
        ValueData::Integer(i) => {
            // Handle overflow: -i64::MIN would overflow, return empty collection per FHIRPath spec
            match i.checked_neg() {
                Some(negated) => Ok(Collection::singleton(Value::integer(negated))),
                None => Ok(Collection::empty()), // Overflow results in empty collection
            }
        }
        ValueData::Decimal(d) => Ok(Collection::singleton(Value::decimal(-d))),
        _ => Err(Error::TypeError("Unary minus requires numeric type".into())),
    }
}

/// Compiled execution plan (bytecode)
#[derive(Debug, Clone)]
pub struct Plan {
    /// Opcodes to execute
    pub opcodes: Vec<Opcode>,

    /// Maximum VM stack depth (collections) for this plan.
    pub max_stack_depth: u16,

    /// Constant pool (literal values)
    pub constants: Vec<crate::value::Value>,

    /// String pool (field names, etc.)
    pub segments: Vec<std::sync::Arc<str>>,

    /// Type specifiers pool (for type operations)
    pub type_specifiers: Vec<String>,

    /// Function IDs referenced
    pub functions: Vec<FunctionId>,

    /// Subplans for closures (where, select, etc.)
    pub subplans: Vec<Plan>,

    /// Variable names indexed by VariableId (3+). Entries may be None for unused slots.
    pub variables: Vec<Option<Arc<str>>>,
}

/// VM opcodes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Opcode {
    // Stack operations
    PushConst(u16),    // Push constant from pool
    PushVariable(u16), // Push variable
    LoadThis,          // Load $this
    LoadIndex,         // Load $index
    LoadTotal,         // Load $total (for aggregate)
    Pop,               // Pop stack
    Dup,               // Duplicate top of stack

    // Navigation
    Navigate(u16), // Navigate to field (index into segments)
    Index(u16),    // Index into collection

    // Operators
    CallBinary(u16), // Call binary operator (impl_id)
    CallUnary(u8),   // Call unary operator (0=plus, 1=minus)
    TypeIs(u16),     // Type check 'is' (index into type_specifiers)
    TypeAs(u16),     // Type cast 'as' (index into type_specifiers)

    // Functions
    CallFunction(u16, u8), // Call function (func_id, arg_count)

    // Higher-order operations
    Where(usize),                    // Where with subplan index
    Select(usize),                   // Select with subplan index
    Repeat(usize),                   // Repeat with subplan index
    Aggregate(usize, Option<usize>), // Aggregate with aggregator subplan index and optional init value subplan index
    Exists(Option<usize>),           // exists() with optional predicate subplan
    All(usize),                      // all(predicate) with predicate subplan

    // Control flow
    Jump(usize),                      // Unconditional jump
    JumpIfEmpty(usize),               // Jump if collection empty
    JumpIfNotEmpty(usize),            // Jump if collection not empty
    Iif(usize, usize, Option<usize>), // Lazy iif(predicate, true, false?)

    // Return
    Return,
}

/// Type alias for function IDs
pub type FunctionId = u16;

/// VM runtime for executing plans
pub struct Vm<'a> {
    stack: Vec<Collection>,
    ctx: &'a Context,
    engine: &'a crate::engine::Engine,
    total: Option<Collection>,           // $total for aggregate() function
    current_path: Option<Vec<Arc<str>>>, // Current navigation path segments for type inference
    resource_type_name: Option<String>,  // Cached root resource/complex type name
}

impl<'a> Vm<'a> {
    pub fn new(ctx: &'a Context, engine: &'a crate::engine::Engine) -> Self {
        Self {
            stack: Vec::new(),
            ctx,
            engine,
            total: None,
            current_path: None,
            resource_type_name: Self::infer_resource_type_name(&ctx.resource),
        }
    }

    /// Create a VM for predicate/projection evaluation (not root navigation)
    pub fn new_for_predicate(ctx: &'a Context, engine: &'a crate::engine::Engine) -> Self {
        Self {
            stack: Vec::new(),
            ctx,
            engine,
            total: None,
            current_path: Some(Vec::new()), // Empty path segments, not root
            resource_type_name: Self::infer_resource_type_name(&ctx.resource),
        }
    }

    fn infer_resource_type_name(resource: &Value) -> Option<String> {
        match resource.data() {
            // OPTIMIZATION: Extract resourceType from LazyJson without materializing
            ValueData::LazyJson { .. } => match resource.data().resolved_json()? {
                JsonValue::Object(obj) => obj
                    .get("resourceType")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        infer_structural_root_type_name_from_json(obj).map(|s| s.to_string())
                    }),
                _ => None,
            },
            ValueData::Object(obj_map) => obj_map
                .get("resourceType")
                .and_then(|col| col.iter().next())
                .and_then(|value| match value.data() {
                    ValueData::String(rt) => Some(rt.as_ref().to_string()),
                    _ => None,
                })
                .or_else(|| {
                    infer_structural_root_type_name(obj_map.as_ref()).map(|s| s.to_string())
                }),
            _ => None,
        }
    }

    /// Convert path segments to a string representation (only when needed)
    fn path_to_string(&self) -> Option<String> {
        self.current_path.as_ref().map(|segments| {
            if segments.is_empty() {
                String::new()
            } else {
                segments.join(".")
            }
        })
    }

    /// Convert path segments to a string slice (for passing to functions)
    fn path_as_str(&self) -> Option<String> {
        self.path_to_string()
    }

    /// Set $total for aggregate() function
    pub fn set_total(&mut self, total: Collection) {
        self.total = Some(total);
    }

    /// Check if a type name is a supertype (parent) of another type by walking up the baseDefinition chain
    fn is_supertype(&self, child_type: &str, parent_type: &str) -> bool {
        // Exact match means it's a supertype (itself)
        if child_type == parent_type {
            return true;
        }

        // Common FHIR base types that are supertypes of all resources
        // These work even without a full FHIR context
        let common_base_types = ["Resource", "DomainResource"];
        if common_base_types.contains(&parent_type) {
            // If parent_type is a common base type, assume it's a supertype
            // This allows expressions like Resource.meta.lastUpdated to work even without full context
            return true;
        }
        false
    }

    /// Execute a plan
    pub fn execute(&mut self, plan: &Plan) -> Result<Collection> {
        self.stack.clear();
        let desired = plan.max_stack_depth as usize;
        if desired > self.stack.capacity() {
            self.stack.reserve(desired - self.stack.capacity());
        }

        // Reset current_path at the start of execution to ensure clean state
        // This is important for union expressions where multiple paths are evaluated
        self.current_path = None;
        let mut ip = 0;
        let max_instructions = plan.opcodes.len() * 1000; // Safety limit to prevent infinite loops
        let mut instruction_count = 0;

        while ip < plan.opcodes.len() {
            instruction_count += 1;
            if instruction_count > max_instructions {
                return Err(Error::EvaluationError(format!(
                    "Execution exceeded maximum instruction limit ({})",
                    max_instructions
                )));
            }
            match plan.opcodes[ip] {
                // Stack operations
                Opcode::PushConst(idx) => {
                    let value = &plan.constants[idx as usize];
                    // Special case: Empty value represents {} (empty collection)
                    // Not a singleton with one Empty value
                    if matches!(value.data(), crate::value::ValueData::Empty) {
                        self.stack.push(Collection::empty());
                    } else {
                        self.stack.push(Collection::singleton(value.clone()));
                    }
                    ip += 1;
                }
                Opcode::PushVariable(var_id) => {
                    // Variable IDs:
                    // 0 = $this (handled by LoadThis)
                    // 1 = $index (handled by LoadIndex)
                    // 2 = $total
                    // 3+ = external constants (%resource, %context, etc.)
                    match var_id {
                        2 => {
                            // $total - get from VM's total field
                            if let Some(total) = &self.total {
                                self.stack.push(total.clone());
                            } else {
                                self.stack.push(Collection::empty());
                            }
                        }
                        0 => {
                            // $this
                            if let Some(this) = &self.ctx.this {
                                self.stack.push(Collection::singleton(this.clone()));
                            } else {
                                self.stack
                                    .push(Collection::singleton(self.ctx.resource.clone()));
                            }
                        }
                        1 => {
                            // $index
                            if let Some(idx) = self.ctx.index {
                                self.stack
                                    .push(Collection::singleton(Value::integer(idx as i64)));
                            } else {
                                self.stack.push(Collection::empty());
                            }
                        }
                        _ => {
                            // External constants - lookup in context variables
                            if let Some(Some(name)) = plan.variables.get(var_id as usize) {
                                if let Some(value) = self.ctx.variables.get(name.as_ref()) {
                                    self.stack.push(Collection::singleton(value.clone()));
                                } else {
                                    // Also try with leading '%' to support both naming styles
                                    let prefixed = format!("%{}", name.as_ref());
                                    if let Some(value) = self.ctx.variables.get(prefixed.as_str()) {
                                        self.stack.push(Collection::singleton(value.clone()));
                                    } else {
                                        self.stack.push(Collection::empty());
                                    }
                                }
                            } else {
                                self.stack.push(Collection::empty());
                            }
                        }
                    }
                    ip += 1;
                }
                Opcode::LoadThis => {
                    // Per FHIRPath spec: $this refers to the current item in iteration.
                    // If not in an iteration context, $this refers to the root resource.
                    if let Some(this) = &self.ctx.this {
                        self.stack.push(Collection::singleton(this.clone()));
                    } else {
                        // Not in iteration context - use root resource
                        self.stack
                            .push(Collection::singleton(self.ctx.resource.clone()));
                    }
                    ip += 1;
                }
                Opcode::LoadIndex => {
                    if let Some(idx) = self.ctx.index {
                        self.stack
                            .push(Collection::singleton(Value::integer(idx as i64)));
                    } else {
                        self.stack.push(Collection::empty());
                    }
                    ip += 1;
                }
                Opcode::LoadTotal => {
                    // Load $total from VM's total field
                    if let Some(total) = &self.total {
                        self.stack.push(total.clone());
                    } else {
                        self.stack.push(Collection::empty());
                    }
                    ip += 1;
                }
                Opcode::Pop => {
                    self.stack
                        .pop()
                        .ok_or_else(|| Error::EvaluationError("Stack underflow on Pop".into()))?;
                    ip += 1;
                }
                Opcode::Dup => {
                    let top = self
                        .stack
                        .last()
                        .ok_or_else(|| Error::EvaluationError("Stack underflow on Dup".into()))?
                        .clone();
                    self.stack.push(top);
                    ip += 1;
                }

                // Navigation
                Opcode::Navigate(seg_idx) => {
                    let field_name = &plan.segments[seg_idx as usize];

                    // Reset current_path if stack is empty - indicates new path expression (e.g., in unions)
                    if self.stack.is_empty() {
                        self.current_path = None;
                    }

                    let is_root_navigation = self.current_path.is_none();

                    // For root navigation, we need the resource on the stack
                    // If stack is empty or we're at root, use resource directly
                    let (collection, popped_from_stack) =
                        if is_root_navigation && self.stack.is_empty() {
                            // Stack is empty but we're at root - use resource directly
                            (Collection::singleton(self.ctx.resource.clone()), false)
                        } else {
                            let col = self.stack.pop().ok_or_else(|| {
                                Error::EvaluationError("Stack underflow on Navigate".into())
                            })?;
                            (col, true)
                        };

                    // Check if we're navigating from the resource itself
                    // This happens when starting a new path expression (e.g., in unions)
                    let is_navigating_from_resource = collection.len() == 1
                        && collection
                            .iter()
                            .next()
                            .map(|v| v.ptr_eq(&self.ctx.resource))
                            .unwrap_or(false);

                    let actual_collection = collection;

                    // If we're navigating from the resource and current_path is set,
                    // this indicates we're starting a new path expression (e.g., right operand of union)
                    // Reset current_path to ensure clean state
                    if is_navigating_from_resource && self.current_path.is_some() {
                        self.current_path = None;
                    }

                    // Per FHIRPath spec: When resolving an identifier at the root of a path,
                    // if it's a type name matching the resource type (or a supertype), skip it.
                    // Check at root navigation (no current_path means we're starting from root)
                    // AND only for FHIR resource/complex types, not System types
                    //
                    // OPTIMIZATION: This type navigation check is expensive (creates TypeRegistry,
                    // checks supertype recursively, scans ahead in opcodes). Only do it when absolutely
                    // necessary - when we're at root navigation AND the field name could plausibly be
                    // a type name (starts with uppercase).
                    let is_root_navigation = self.current_path.is_none();

                    // Type name navigation only applies when the identifier starts with an
                    // uppercase letter OR exactly matches the resource type (case-sensitive).
                    // Field names like "extension" should NOT trigger type navigation even if
                    // the inferred type name matches case-insensitively ("Extension").
                    let should_try_type_navigation = is_root_navigation
                        && (field_name
                            .as_ref()
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_uppercase())
                            || self
                                .resource_type_name
                                .as_deref()
                                .is_some_and(|rt| rt == field_name.as_ref()));

                    if should_try_type_navigation {
                        // Check if type name matches resource type exactly or is a supertype
                        let resource_type_matches =
                            if let Some(rt) = self.resource_type_name.as_deref() {
                                rt.eq_ignore_ascii_case(field_name.as_ref())
                                    || self.is_supertype(rt, field_name.as_ref())
                            } else {
                                false
                            };

                        // Only treat as type navigation if it matches the resource type or is a supertype
                        // Per spec: "When resolving an identifier at the root of a path, it is resolved as a type name first
                        // (only for FHIR types), and if it resolves to a type, it must resolve to the type of the context (or a supertype)"
                        // So we skip if resource_type_matches is true (exact match or supertype)
                        if resource_type_matches {
                            // Type matches the root context: treat this identifier as a type name and
                            // skip it (it is a no-op filter for singleton-root evaluation).
                            //
                            // Always keep the root resource on the stack so subsequent navigation,
                            // method calls, and binary ops operate on the correct base.
                            self.current_path = Some(Vec::new());
                            self.stack
                                .push(Collection::singleton(self.ctx.resource.clone()));
                            ip += 1;
                            continue;
                        }
                        // If it's not a type name matching the resource, proceed with normal field navigation
                        // Note: If it's a FHIR type name that doesn't match, navigating it as a field will
                        // return empty (field doesn't exist), which is correct per spec
                    } else {
                        // Continue building path - use the collection that was popped
                        // For choice types, we need to pass the path BEFORE adding the field name
                        // so that choice expansion can check "Observation.value[x]" not "Observation.value.unit[x]"
                        let path_before_field = self.current_path.as_deref();

                        // Proceed with normal navigation using path BEFORE field name
                        // This allows choice expansion to work correctly (e.g., "Observation.value[x]")
                        let (result, resolved_segment) =
                            self.navigate_field(actual_collection, field_name, path_before_field)?;
                        let seg = resolved_segment.unwrap_or_else(|| field_name.clone());
                        if let Some(ref mut path_segments) = self.current_path {
                            path_segments.push(seg);
                        } else {
                            self.current_path = Some(vec![seg]);
                        }
                        self.stack.push(result);
                        ip += 1;
                        continue;
                    }

                    // Proceed with normal navigation (for root navigation case)
                    // Use actual_collection which is the collection popped from stack
                    // For root navigation where nothing was on stack, use resource explicitly
                    // If we popped from stack, use what we popped (could be TypeInfo or other intermediate result)
                    let collection_to_navigate = if is_root_navigation && !popped_from_stack {
                        // Nothing on stack and at root - use resource
                        Collection::singleton(self.ctx.resource.clone())
                    } else {
                        // Either not at root, or we popped something from stack - use it
                        actual_collection
                    };
                    let path_segments = self.current_path.as_deref();
                    let (result, resolved_segment) =
                        self.navigate_field(collection_to_navigate, field_name, path_segments)?;
                    let seg = resolved_segment.unwrap_or_else(|| field_name.clone());
                    if let Some(ref mut path_segments) = self.current_path {
                        path_segments.push(seg);
                    } else {
                        self.current_path = Some(vec![seg]);
                    }
                    self.stack.push(result);
                    ip += 1;
                }
                Opcode::Index(idx) => {
                    let collection = self
                        .stack
                        .pop()
                        .ok_or_else(|| Error::EvaluationError("Stack underflow on Index".into()))?;

                    let result = self.index_collection(collection, idx as usize)?;
                    self.stack.push(result);
                    ip += 1;
                }

                // Operators
                Opcode::CallBinary(impl_id) => {
                    let right = self.stack.pop().ok_or_else(|| {
                        Error::EvaluationError("Stack underflow on CallBinary".into())
                    })?;
                    let left = self.stack.pop().ok_or_else(|| {
                        Error::EvaluationError("Stack underflow on CallBinary".into())
                    })?;

                    // Map impl_id to operator
                    let op = self.map_impl_id_to_operator(impl_id)?;
                    let result = execute_binary_op(op, left, right)?;
                    self.stack.push(result);
                    // Reset current_path after binary operation to ensure clean state for subsequent path evaluations
                    // This is important for nested expressions and unions
                    self.current_path = None;
                    ip += 1;
                }
                Opcode::CallUnary(unary_op) => {
                    let operand = self.stack.pop().ok_or_else(|| {
                        Error::EvaluationError("Stack underflow on CallUnary".into())
                    })?;

                    let result = match unary_op {
                        0 => unary_plus(operand)?,  // Plus
                        1 => unary_minus(operand)?, // Minus
                        _ => {
                            return Err(Error::Unsupported(format!(
                                "Unknown unary operator: {}",
                                unary_op
                            )));
                        }
                    };
                    self.stack.push(result);
                    ip += 1;
                }
                Opcode::TypeIs(type_idx) => {
                    let collection = self.stack.pop().ok_or_else(|| {
                        Error::EvaluationError("Stack underflow on TypeIs".into())
                    })?;

                    let type_spec = &plan.type_specifiers[type_idx as usize];
                    let result = self.type_is(collection, type_spec)?;
                    self.stack.push(result);
                    ip += 1;
                }
                Opcode::TypeAs(type_idx) => {
                    let collection = self.stack.pop().ok_or_else(|| {
                        Error::EvaluationError("Stack underflow on TypeAs".into())
                    })?;

                    let type_spec = &plan.type_specifiers[type_idx as usize];
                    let result = self.type_as(collection, type_spec)?;
                    self.stack.push(result);
                    ip += 1;
                }

                // Functions
                Opcode::CallFunction(func_id, arg_count) => {
                    // Pop arguments from stack (in reverse order)
                    let mut args = Vec::new();
                    for _ in 0..arg_count {
                        args.push(self.stack.pop().ok_or_else(|| {
                            Error::EvaluationError("Stack underflow on CallFunction".into())
                        })?);
                    }
                    args.reverse(); // Reverse to get correct order

                    // Get collection from stack (the collection the function is called on)
                    // For standalone functions (now, today, timeOfDay), collection is not needed
                    // If stack is empty, use empty collection (for standalone function calls)
                    let collection = self.stack.pop().unwrap_or_else(Collection::empty);

                    // Execute function
                    let path_str = self.path_as_str();
                    let result = execute_function(
                        func_id,
                        collection,
                        args,
                        self.ctx,
                        path_str.as_deref(),
                        Some(self.engine.fhir_context().as_ref()),
                        self.engine.resource_resolver(),
                    )?;
                    self.stack.push(result);
                    ip += 1;
                }

                // Higher-order operations
                Opcode::Where(subplan_idx) => {
                    let collection = self
                        .stack
                        .pop()
                        .ok_or_else(|| Error::EvaluationError("Stack underflow on Where".into()))?;

                    let subplan = &plan.subplans[subplan_idx];
                    let result = self.execute_where(collection, subplan)?;
                    self.stack.push(result);
                    ip += 1;
                }
                Opcode::Select(subplan_idx) => {
                    let collection = self.stack.pop().ok_or_else(|| {
                        Error::EvaluationError("Stack underflow on Select".into())
                    })?;

                    let subplan = &plan.subplans[subplan_idx];
                    let result = self.execute_select(collection, subplan)?;
                    self.stack.push(result);
                    ip += 1;
                }
                Opcode::Repeat(subplan_idx) => {
                    let collection = self.stack.pop().ok_or_else(|| {
                        Error::EvaluationError("Stack underflow on Repeat".into())
                    })?;

                    let subplan = &plan.subplans[subplan_idx];
                    let result = self.execute_repeat(collection, subplan)?;
                    self.stack.push(result);
                    ip += 1;
                }
                Opcode::Aggregate(aggregator_idx, init_value_idx) => {
                    let collection = self.stack.pop().ok_or_else(|| {
                        Error::EvaluationError("Stack underflow on Aggregate".into())
                    })?;

                    let aggregator_plan = &plan.subplans[aggregator_idx];
                    let init_value_plan = init_value_idx.map(|idx| &plan.subplans[idx]);
                    let result =
                        self.execute_aggregate(collection, aggregator_plan, init_value_plan)?;
                    self.stack.push(result);
                    ip += 1;
                }
                Opcode::Exists(subplan_idx) => {
                    let collection = self.stack.pop().ok_or_else(|| {
                        Error::EvaluationError("Stack underflow on Exists".into())
                    })?;

                    let mut any = false;

                    if let Some(pred_idx) = subplan_idx {
                        let predicate_plan = &plan.subplans[pred_idx];

                        for (index, item) in collection.iter().enumerate() {
                            let item_context = Context {
                                this: Some(item.clone()),
                                index: Some(index),
                                strict: self.ctx.strict,
                                variables: self.ctx.variables.clone(),
                                resource: self.ctx.resource.clone(),
                                root: self.ctx.root.clone(),
                            };

                            let mut item_vm = Vm::new_for_predicate(&item_context, self.engine);
                            let predicate_result = item_vm.execute(predicate_plan)?;

                            let matches = if predicate_result.is_empty() {
                                false
                            } else {
                                predicate_result
                                    .as_boolean()
                                    .unwrap_or_else(|_| !predicate_result.is_empty())
                            };

                            if matches {
                                any = true;
                                break;
                            }
                        }
                    } else {
                        any = !collection.is_empty();
                    }

                    self.stack.push(Collection::singleton(Value::boolean(any)));
                    ip += 1;
                }

                Opcode::All(pred_idx) => {
                    let collection = self
                        .stack
                        .pop()
                        .ok_or_else(|| Error::EvaluationError("Stack underflow on All".into()))?;

                    // Per FHIRPath: all() over empty collection is true.
                    if collection.is_empty() {
                        self.stack.push(Collection::singleton(Value::boolean(true)));
                        ip += 1;
                        continue;
                    }

                    let predicate_plan = &plan.subplans[pred_idx];
                    let mut all_true = true;

                    for (index, item) in collection.iter().enumerate() {
                        let item_context = Context {
                            this: Some(item.clone()),
                            index: Some(index),
                            strict: self.ctx.strict,
                            variables: self.ctx.variables.clone(),
                            resource: self.ctx.resource.clone(),
                            root: self.ctx.root.clone(),
                        };

                        let mut item_vm = Vm::new_for_predicate(&item_context, self.engine);
                        item_vm.total = self.total.clone();
                        let predicate_result = item_vm.execute(predicate_plan)?;

                        if predicate_result.is_empty() {
                            all_true = false;
                            break;
                        }

                        // Predicate must be boolean singleton.
                        if predicate_result.len() != 1 {
                            return Err(Error::TypeError(
                                "all() predicate must evaluate to a boolean".into(),
                            ));
                        }
                        let b = predicate_result.as_boolean().map_err(|_| {
                            Error::TypeError("all() predicate must evaluate to a boolean".into())
                        })?;
                        if !b {
                            all_true = false;
                            break;
                        }
                    }

                    self.stack
                        .push(Collection::singleton(Value::boolean(all_true)));
                    ip += 1;
                }

                // Control flow
                Opcode::Jump(target_ip) => {
                    ip = target_ip;
                }
                Opcode::JumpIfEmpty(target_ip) => {
                    let top = self.stack.last().ok_or_else(|| {
                        Error::EvaluationError("Stack underflow on JumpIfEmpty".into())
                    })?;
                    if top.is_empty() {
                        ip = target_ip;
                    } else {
                        ip += 1;
                    }
                }
                Opcode::JumpIfNotEmpty(target_ip) => {
                    let top = self.stack.last().ok_or_else(|| {
                        Error::EvaluationError("Stack underflow on JumpIfNotEmpty".into())
                    })?;
                    if !top.is_empty() {
                        ip = target_ip;
                    } else {
                        ip += 1;
                    }
                }
                Opcode::Iif(predicate_idx, true_idx, false_idx) => {
                    // Input collection for iif (method-call form)
                    let input_collection = self.stack.pop().unwrap_or_else(Collection::empty);
                    if input_collection.len() > 1 {
                        return Err(Error::TypeError(
                            "iif() requires singleton input collection".into(),
                        ));
                    }

                    let this_value = input_collection.iter().next().cloned();
                    let local_ctx = Context {
                        this: this_value,
                        index: self.ctx.index,
                        strict: self.ctx.strict,
                        variables: self.ctx.variables.clone(),
                        resource: self.ctx.resource.clone(),
                        root: self.ctx.root.clone(),
                    };

                    // Evaluate predicate
                    let predicate_plan = &plan.subplans[predicate_idx];
                    let mut pred_vm = Vm::new(&local_ctx, self.engine);
                    pred_vm.total = self.total.clone();
                    pred_vm.current_path = self.current_path.clone();
                    let pred_result = pred_vm.execute(predicate_plan)?;

                    if !pred_result.is_empty() && pred_result.len() > 1 {
                        return Err(Error::TypeError(
                            "iif() criterion must be empty or singleton".into(),
                        ));
                    }

                    let predicate_bool = if pred_result.is_empty() {
                        None
                    } else {
                        Some(pred_result.as_boolean().map_err(|_| {
                            Error::TypeError("iif() criterion must be a boolean".into())
                        })?)
                    };

                    let branch_plan = if predicate_bool == Some(true) {
                        Some(&plan.subplans[true_idx])
                    } else {
                        false_idx.map(|idx| &plan.subplans[idx])
                    };

                    if let Some(chosen) = branch_plan {
                        let mut branch_vm = Vm::new(&local_ctx, self.engine);
                        branch_vm.total = self.total.clone();
                        branch_vm.current_path = self.current_path.clone();
                        let branch_result = branch_vm.execute(chosen)?;
                        self.stack.push(branch_result);
                    } else {
                        self.stack.push(Collection::empty());
                    }

                    ip += 1;
                }

                // Return
                Opcode::Return => {
                    return self
                        .stack
                        .pop()
                        .ok_or_else(|| Error::EvaluationError("Stack underflow on Return".into()));
                }
            }
        }

        Err(Error::EvaluationError("Plan did not return".into()))
    }

    /// Navigate to a field in a collection
    ///
    /// `path` is the current navigation path segments (e.g., ["Patient","name"]) for strict errors.
    fn navigate_field(
        &self,
        collection: Collection,
        field_name: &Arc<str>,
        path: Option<&[Arc<str>]>,
    ) -> Result<(Collection, Option<Arc<str>>)> {
        fn format_path(base: Option<&[Arc<str>]>, leaf: &str) -> String {
            match base {
                Some(segments) if !segments.is_empty() => {
                    let mut out = String::new();
                    for (i, seg) in segments.iter().enumerate() {
                        if i > 0 {
                            out.push('.');
                        }
                        out.push_str(seg.as_ref());
                    }
                    out.push('.');
                    out.push_str(leaf);
                    out
                }
                _ => leaf.to_string(),
            }
        }

        fn is_choice_variant_key(key: &str, base: &str) -> bool {
            let kb = key.as_bytes();
            let bb = base.as_bytes();
            if kb.len() <= bb.len() || !key.starts_with(base) {
                return false;
            }
            let next = kb[bb.len()];
            next.is_ascii_uppercase()
        }

        let mut result = Collection::empty();
        let mut resolved_segment: Option<Arc<str>> = None;
        let mut found = false;

        // In strict mode, disallow direct access to choice-type expanded names (e.g., valueQuantity)
        if self.ctx.strict {
            let fname: &str = field_name.as_ref();
            if fname
                .as_bytes()
                .iter()
                .skip(1)
                .any(|b| b.is_ascii_uppercase())
            {
                let full_path = format_path(path, fname);
                return Err(Error::TypeError(format!(
                    "Path '{}' does not exist on current context",
                    full_path
                )));
            }
        }

        for item in collection.iter() {
            match item.data() {
                // OPTIMIZATION: Handle lazy JSON without materializing the entire object
                ValueData::LazyJson { root, path } => {
                    let Some(JsonValue::Object(obj)) = item.data().resolved_json() else {
                        continue;
                    };

                    let mut base_path = path.clone();

                    // Direct field access on raw JSON - O(1) lookup, no conversion of unaccessed fields
                    if let Some(field_value) = obj.get(field_name.as_ref()) {
                        base_path.push(crate::value::JsonPathToken::Key(field_name.clone()));
                        match field_value {
                            JsonValue::Array(arr) => {
                                for (idx, child) in arr.iter().enumerate() {
                                    let mut child_path = base_path.clone();
                                    child_path.push(crate::value::JsonPathToken::Index(idx));
                                    result.push(Value::from_json_node(
                                        root.clone(),
                                        child_path,
                                        child,
                                    ));
                                }
                            }
                            other => {
                                result.push(Value::from_json_node(root.clone(), base_path, other));
                            }
                        }
                        found = true;
                        continue;
                    }

                    // Check for choice types by scanning keys
                    let base = field_name.as_ref();
                    for (key, field_value) in obj.iter() {
                        let key_str = key.as_str();
                        if !is_choice_variant_key(key_str, base) {
                            continue;
                        }

                        let chosen: Arc<str> = Arc::from(key_str);
                        resolved_segment = Some(chosen.clone());
                        base_path.push(crate::value::JsonPathToken::Key(chosen));

                        match field_value {
                            JsonValue::Array(arr) => {
                                for (idx, child) in arr.iter().enumerate() {
                                    let mut child_path = base_path.clone();
                                    child_path.push(crate::value::JsonPathToken::Index(idx));
                                    result.push(Value::from_json_node(
                                        root.clone(),
                                        child_path,
                                        child,
                                    ));
                                }
                            }
                            other => {
                                result.push(Value::from_json_node(root.clone(), base_path, other));
                            }
                        }
                        found = true;
                        break;
                    }
                }
                ValueData::Object(obj) => {
                    // Materialized object - use existing logic
                    // First, try direct field access
                    if let Some(field_collection) = obj.get(field_name.as_ref()) {
                        // Add all items from the field collection
                        // This handles both single values and arrays
                        for field_item in field_collection.iter() {
                            result.push(field_item.clone());
                        }
                        found = true;
                    } else {
                        // Field not found directly - check if it's a choice type
                        // Choice types have [x] in the path, which gets expanded at runtime
                        // e.g., Observation.value[x] becomes Observation.valueQuantity or Observation.valueString
                        // We dynamically check all fields that start with the base field name
                        // This is a runtime check based on actual data structure
                        let base = field_name.as_ref();
                        for key in obj.keys() {
                            if is_choice_variant_key(key.as_ref(), base) {
                                // Check if this is a valid choice variant
                                // e.g., "valueQuantity" starts with "value" and has more characters
                                // The next character should be uppercase (camelCase)
                                // This looks like a choice variant (e.g., "valueQuantity")
                                if let Some(field_collection) = obj.get(key) {
                                    resolved_segment = Some(key.clone());
                                    for field_item in field_collection.iter() {
                                        result.push(field_item.clone());
                                    }
                                    found = true;
                                    break; // Found it, no need to check other types
                                }
                            }
                        }
                        // If no choice type found and field doesn't exist, result remains empty
                        // This is correct FHIRPath behavior - missing fields return empty collection
                    }
                }
                _ => {
                    // Not an object, field access returns empty
                    // This is correct FHIRPath behavior
                }
            }
        }

        // In strict mode, unknown fields should raise errors when the base collection was non-empty
        if self.ctx.strict && !found && !collection.is_empty() {
            let full_path = format_path(path, field_name.as_ref());
            return Err(Error::TypeError(format!(
                "Path '{}' does not exist on current context",
                full_path
            )));
        }

        Ok((result, resolved_segment))
    }

    /// Index into a collection
    fn index_collection(&self, collection: Collection, index: usize) -> Result<Collection> {
        if collection.is_empty() {
            return Ok(Collection::empty());
        }

        Ok(collection
            .get(index)
            .cloned()
            .map(Collection::singleton)
            .unwrap_or_else(Collection::empty))
    }

    /// Map implementation ID to binary operator
    fn map_impl_id_to_operator(&self, impl_id: u16) -> Result<HirBinaryOperator> {
        // Map impl_id to operator (matches encoding in codegen)
        match impl_id {
            0 => Ok(HirBinaryOperator::Add),
            1 => Ok(HirBinaryOperator::Sub),
            2 => Ok(HirBinaryOperator::Mul),
            3 => Ok(HirBinaryOperator::Div),
            4 => Ok(HirBinaryOperator::Mod),
            5 => Ok(HirBinaryOperator::DivInt),
            10 => Ok(HirBinaryOperator::Eq),
            11 => Ok(HirBinaryOperator::Ne),
            12 => Ok(HirBinaryOperator::Equivalent),
            13 => Ok(HirBinaryOperator::NotEquivalent),
            20 => Ok(HirBinaryOperator::Lt),
            21 => Ok(HirBinaryOperator::Le),
            22 => Ok(HirBinaryOperator::Gt),
            23 => Ok(HirBinaryOperator::Ge),
            30 => Ok(HirBinaryOperator::And),
            31 => Ok(HirBinaryOperator::Or),
            32 => Ok(HirBinaryOperator::Xor),
            33 => Ok(HirBinaryOperator::Implies),
            40 => Ok(HirBinaryOperator::Union),
            41 => Ok(HirBinaryOperator::In),
            42 => Ok(HirBinaryOperator::Contains),
            50 => Ok(HirBinaryOperator::Concat),
            _ => Err(Error::Unsupported(format!(
                "Unknown operator impl_id: {}",
                impl_id
            ))),
        }
    }

    /// Execute where clause with predicate subplan
    fn execute_where(
        &mut self,
        collection: Collection,
        predicate_plan: &Plan,
    ) -> Result<Collection> {
        let mut result = Collection::empty();

        for (index, item) in collection.iter().enumerate() {
            // Create new context with $this and $index
            let item_context = Context {
                this: Some(item.clone()),
                index: Some(index),
                strict: self.ctx.strict,
                variables: self.ctx.variables.clone(),
                resource: self.ctx.resource.clone(),
                root: self.ctx.root.clone(),
            };

            // Execute predicate subplan
            let mut item_vm = Vm::new_for_predicate(&item_context, self.engine);
            let predicate_result = match item_vm.execute(predicate_plan) {
                Ok(res) => res,
                Err(Error::TypeError(msg)) if msg.contains("Empty collection") => {
                    // Treat empty/missing predicate result as false per FHIRPath truthiness
                    Collection::empty()
                }
                Err(e) => return Err(e),
            };

            // Check if predicate evaluates to true
            // Per FHIRPath spec for where():
            // - Empty collection = false (exclude item)
            // - Boolean true = true (include item)
            // - Boolean false = false (exclude item)
            // - Non-empty, non-boolean collection = error (but we treat as truthy for now)
            let should_include = if predicate_result.is_empty() {
                false
            } else {
                // Try to get boolean value - this is what where() expects
                // Per FHIRPath spec, where() requires predicate to evaluate to boolean
                predicate_result.as_boolean().unwrap_or_else(|_| {
                    // If not a boolean, per spec this should error, but for compatibility
                    // treat non-empty collection as truthy
                    !predicate_result.is_empty()
                })
            };

            if should_include {
                result.push(item.clone());
            }
        }

        Ok(result)
    }

    /// Execute select clause with projection subplan
    fn execute_select(
        &mut self,
        collection: Collection,
        projection_plan: &Plan,
    ) -> Result<Collection> {
        let mut result = Collection::empty();

        for (index, item) in collection.iter().enumerate() {
            // Create new context with $this and $index
            let item_context = Context {
                this: Some(item.clone()),
                index: Some(index),
                strict: self.ctx.strict,
                variables: self.ctx.variables.clone(),
                resource: self.ctx.resource.clone(),
                root: self.ctx.root.clone(),
            };

            // Execute projection subplan
            let mut item_vm = Vm::new_for_predicate(&item_context, self.engine);
            let projection_result = item_vm.execute(projection_plan)?;

            // Add all items from projection result
            for projected_item in projection_result.iter() {
                result.push(projected_item.clone());
            }
        }

        Ok(result)
    }

    /// Execute repeat clause with projection subplan and cycle detection
    fn execute_repeat(
        &mut self,
        collection: Collection,
        projection_plan: &Plan,
    ) -> Result<Collection> {
        if collection.is_empty() {
            return Ok(Collection::empty());
        }

        // Input queue: items to process
        let mut input_queue: Vec<Value> = collection.iter().cloned().collect();
        // Output collection: unique items found
        let mut result = Collection::empty();
        // Seen items: track items already in output (cycle detection)
        let mut seen_items = Vec::new();
        // Safety limit to prevent infinite loops
        const MAX_ITERATIONS: usize = 10000;
        let mut iterations = 0;

        // Helper to check if item is already seen using FHIRPath equality semantics.
        let is_seen = |item: &Value, seen: &[Value]| -> bool {
            seen.iter().any(|seen_item| {
                let left = Collection::singleton(item.clone());
                let right = Collection::singleton(seen_item.clone());
                match execute_binary_op(HirBinaryOperator::Eq, left, right) {
                    Ok(res) => res.as_boolean().unwrap_or(false),
                    Err(_) => false,
                }
            })
        };

        // Process queue until empty (with safety limit)
        while let Some(current_item) = input_queue.pop() {
            iterations += 1;
            if iterations > MAX_ITERATIONS {
                return Err(Error::EvaluationError(format!(
                    "repeat() exceeded maximum iterations ({}) - possible infinite loop",
                    MAX_ITERATIONS
                )));
            }

            // Create context with $this = current_item
            let item_context = Context {
                this: Some(current_item.clone()),
                index: None,
                strict: self.ctx.strict,
                variables: self.ctx.variables.clone(),
                resource: self.ctx.resource.clone(),
                root: self.ctx.root.clone(),
            };

            // Execute projection subplan
            let mut item_vm = Vm::new_for_predicate(&item_context, self.engine);
            let projection_result = item_vm.execute(projection_plan)?;

            // Process all items from projection result
            for new_item in projection_result.iter() {
                // Check if item is already in output (cycle detection)
                if !is_seen(new_item, &seen_items) {
                    // Add to output and queue for further processing
                    result.push(new_item.clone());
                    seen_items.push(new_item.clone());
                    input_queue.push(new_item.clone());
                }
            }
        }

        Ok(result)
    }

    /// Type check operation (is)
    fn type_is(&self, collection: Collection, type_spec: &str) -> Result<Collection> {
        if collection.is_empty() {
            return Ok(Collection::empty());
        }

        if collection.len() != 1 {
            return Err(Error::TypeError(
                "is() requires singleton collection".into(),
            ));
        }

        functions::validate_type_specifier(type_spec, Some(self.engine.fhir_context().as_ref()))?;

        if self.ctx.strict {
            let path_str = self.path_as_str();
            if let Some(seg_type) = declared_type_from_path(path_str.as_deref()) {
                let spec_base = normalize_type_name(type_spec);
                if !spec_base.is_empty() && !seg_type.eq_ignore_ascii_case(&spec_base) {
                    return Err(Error::TypeError(format!(
                        "Type '{}' is incompatible with element type '{}'",
                        type_spec, seg_type
                    )));
                }
            }
        }

        let item = collection.iter().next().unwrap();
        let path_str = self.path_as_str();
        let matches = functions::matches_type_specifier(
            item,
            type_spec,
            path_str.as_deref(),
            Some(self.engine.fhir_context().as_ref()),
            self.ctx,
        );

        Ok(Collection::singleton(Value::boolean(matches)))
    }

    /// Type cast operation (as)
    ///
    /// IMPORTANT: Deviation from strict FHIRPath spec for FHIR R4 compatibility
    /// -------------------------------------------------------------------------
    /// The FHIRPath spec (6.3.3) states that the `as` operator should throw an error
    /// when the input collection has more than one item. However, FHIR R4 core search
    /// parameter definitions use expressions like:
    ///   (Observation.component.value as Quantity) | (Observation.component.value as SampledData)
    ///
    /// When an Observation has multiple components, Observation.component.value
    /// returns a multi-item collection, which would fail under strict spec interpretation.
    ///
    /// To support FHIR R4 search parameters out-of-the-box, we implement the `as` operator
    /// to filter multi-item collections (similar to ofType()), returning only items
    /// that match the specified type. This approach is consistent with major FHIR
    /// implementations (HAPI FHIR, Microsoft FHIR Server) and enables compatibility
    /// with official FHIR search parameter definitions.
    ///
    /// Note: For strict FHIRPath spec compliance, use `as` only within where() clauses
    /// where each iteration evaluates a singleton, or use ofType() for explicit filtering.
    fn type_as(&self, collection: Collection, type_spec: &str) -> Result<Collection> {
        if collection.is_empty() {
            return Ok(Collection::empty());
        }

        // NOTE: We intentionally do NOT throw an error for multi-item collections here.
        // Instead, we filter the collection to only items matching the type (see above).

        functions::validate_type_specifier(type_spec, Some(self.engine.fhir_context().as_ref()))?;

        if self.ctx.strict {
            let path_str = self.path_as_str();
            if let Some(seg_type) = declared_type_from_path(path_str.as_deref()) {
                let spec_base = normalize_type_name(type_spec);
                if !spec_base.is_empty() && !seg_type.eq_ignore_ascii_case(&spec_base) {
                    return Err(Error::TypeError(format!(
                        "Type '{}' is incompatible with element type '{}'",
                        type_spec, seg_type
                    )));
                }
            }
        }

        // Filter collection to items matching the type (similar to ofType)
        let path_str = self.path_as_str();
        let mut result = Collection::empty();

        for item in collection.iter() {
            // Use exact matching for 'as' operator (no inheritance)
            let matches = functions::matches_type_specifier_exact(
                item,
                type_spec,
                path_str.as_deref(),
                Some(self.engine.fhir_context().as_ref()),
                self.ctx,
            );

            if matches {
                result.push(item.clone());
            }
        }

        Ok(result)
    }

    /// Execute aggregate clause with aggregator subplan
    fn execute_aggregate(
        &mut self,
        collection: Collection,
        aggregator_plan: &Plan,
        init_value_plan: Option<&Plan>,
    ) -> Result<Collection> {
        aggregate_with_subplans(
            collection,
            aggregator_plan,
            init_value_plan,
            self.ctx,
            self.engine,
        )
    }
}

/// Extract a declared type hint from the current navigation path (e.g., valueQuantity → Quantity, birthDate → Date)
fn declared_type_from_path(path_hint: Option<&str>) -> Option<String> {
    let path = path_hint?;
    let segment = path.rsplit('.').find(|s| !s.is_empty())?;
    let mut chars = segment.char_indices().peekable();
    #[allow(clippy::while_let_on_iterator)]
    while let Some((idx, ch)) = chars.next() {
        if ch.is_uppercase() {
            let suffix = &segment[idx..];
            if !suffix.is_empty() {
                return Some(suffix.to_ascii_lowercase());
            }
        }
    }
    None
}

fn normalize_type_name(type_spec: &str) -> String {
    let trimmed = type_spec.trim().trim_start_matches('@');
    let base = trimmed
        .rsplit('.')
        .next()
        .map(|s| s.trim_matches('`'))
        .unwrap_or(trimmed);
    base.to_ascii_lowercase()
}

pub(crate) fn infer_structural_root_type_name(
    obj: &std::collections::HashMap<Arc<str>, Collection>,
) -> Option<&'static str> {
    // Heuristic structural type inference for common FHIR complex datatypes when no resourceType is present.
    let has_choice_value = obj.keys().any(|k| {
        let s = k.as_ref();
        s.starts_with("value") && s.len() > 5 && s.as_bytes()[5].is_ascii_uppercase()
    });

    if obj.contains_key("versionId")
        || obj.contains_key("lastUpdated")
        || obj.contains_key("profile")
        || obj.contains_key("security")
        || obj.contains_key("tag")
        || obj.contains_key("source")
    {
        return Some("Meta");
    }

    if obj.contains_key("div") && obj.contains_key("status") {
        return Some("Narrative");
    }

    if obj.contains_key("url") && (obj.contains_key("extension") || has_choice_value) {
        return Some("Extension");
    }

    if obj.contains_key("contentType")
        && (obj.contains_key("data")
            || obj.contains_key("url")
            || obj.contains_key("size")
            || obj.contains_key("hash")
            || obj.contains_key("title")
            || obj.contains_key("creation"))
    {
        return Some("Attachment");
    }

    if obj.contains_key("text")
        && (obj.contains_key("authorString")
            || obj.contains_key("authorReference")
            || obj.contains_key("time"))
    {
        return Some("Annotation");
    }

    if obj.contains_key("numerator") && obj.contains_key("denominator") {
        return Some("Ratio");
    }

    if obj.contains_key("denominator")
        && (obj.contains_key("lowNumerator") || obj.contains_key("highNumerator"))
    {
        return Some("RatioRange");
    }

    if obj.contains_key("origin") && obj.contains_key("data") && obj.contains_key("dimensions") {
        return Some("SampledData");
    }

    if obj.contains_key("type")
        && obj.contains_key("when")
        && obj.contains_key("who")
        && (obj.contains_key("data") || obj.contains_key("sigFormat"))
    {
        return Some("Signature");
    }

    if obj.contains_key("doseAndRate")
        || (obj.contains_key("timing")
            && (obj.contains_key("route")
                || obj.contains_key("site")
                || obj.contains_key("method")
                || obj.contains_key("asNeededBoolean")
                || obj.contains_key("asNeededCodeableConcept")))
    {
        return Some("Dosage");
    }

    if obj.contains_key("repeat") || obj.contains_key("event") {
        return Some("Timing");
    }

    if obj.contains_key("system")
        && obj.contains_key("value")
        && !obj.contains_key("code")
        && (obj.contains_key("use") || obj.contains_key("rank") || obj.contains_key("period"))
    {
        return Some("ContactPoint");
    }

    if obj.contains_key("line")
        || obj.contains_key("city")
        || obj.contains_key("state")
        || obj.contains_key("postalCode")
        || obj.contains_key("country")
    {
        return Some("Address");
    }

    if obj.contains_key("family")
        || obj.contains_key("given")
        || obj.contains_key("prefix")
        || obj.contains_key("suffix")
    {
        return Some("HumanName");
    }

    let has_numeric_value = obj
        .get("value")
        .and_then(|col| col.iter().next())
        .map(|v| matches!(v.data(), ValueData::Integer(_) | ValueData::Decimal(_)))
        .unwrap_or(false);
    if has_numeric_value && obj.contains_key("currency") {
        return Some("Money");
    }
    let quantity_fields = has_numeric_value
        && (obj.contains_key("unit") || obj.contains_key("code") || obj.contains_key("system"));
    if quantity_fields {
        return Some("Quantity");
    }

    let period_fields = obj.contains_key("start") || obj.contains_key("end");
    if period_fields {
        return Some("Period");
    }

    let coding_fields = (obj.contains_key("code") && obj.contains_key("system"))
        || (obj.contains_key("code") && obj.contains_key("display"));
    if coding_fields {
        return Some("Coding");
    }

    let identifier_fields = obj.contains_key("value") && !obj.contains_key("code");
    if identifier_fields {
        return Some("Identifier");
    }

    let reference_fields = obj.contains_key("reference")
        || (obj.contains_key("identifier") && obj.contains_key("type"))
        || obj.contains_key("display");
    if reference_fields {
        return Some("Reference");
    }

    None
}

pub(crate) fn infer_structural_root_type_name_from_json(
    obj: &serde_json::Map<String, JsonValue>,
) -> Option<&'static str> {
    // Heuristic structural type inference for common FHIR complex datatypes when no resourceType is present.
    let has_choice_value = obj.keys().any(|k| {
        k.starts_with("value")
            && k.len() > 5
            && k.as_bytes().get(5).is_some_and(|b| b.is_ascii_uppercase())
    });

    if obj.contains_key("versionId")
        || obj.contains_key("lastUpdated")
        || obj.contains_key("profile")
        || obj.contains_key("security")
        || obj.contains_key("tag")
        || obj.contains_key("source")
    {
        return Some("Meta");
    }

    if obj.contains_key("div") && obj.contains_key("status") {
        return Some("Narrative");
    }

    if obj.contains_key("url") && (obj.contains_key("extension") || has_choice_value) {
        return Some("Extension");
    }

    if obj.contains_key("contentType")
        && (obj.contains_key("data")
            || obj.contains_key("url")
            || obj.contains_key("size")
            || obj.contains_key("hash")
            || obj.contains_key("title")
            || obj.contains_key("creation"))
    {
        return Some("Attachment");
    }

    if obj.contains_key("text")
        && (obj.contains_key("authorString")
            || obj.contains_key("authorReference")
            || obj.contains_key("time"))
    {
        return Some("Annotation");
    }

    if obj.contains_key("numerator") && obj.contains_key("denominator") {
        return Some("Ratio");
    }

    if obj.contains_key("denominator")
        && (obj.contains_key("lowNumerator") || obj.contains_key("highNumerator"))
    {
        return Some("RatioRange");
    }

    if obj.contains_key("low") && obj.contains_key("high") && obj.contains_key("type") {
        return Some("Range");
    }

    if obj.contains_key("coding") || obj.contains_key("text") {
        return Some("CodeableConcept");
    }

    let has_numeric_value = obj
        .get("value")
        .is_some_and(|v| v.is_i64() || v.is_u64() || v.is_f64());
    if has_numeric_value && obj.contains_key("currency") {
        return Some("Money");
    }
    let quantity_fields = has_numeric_value
        && (obj.contains_key("unit") || obj.contains_key("code") || obj.contains_key("system"));
    if quantity_fields {
        return Some("Quantity");
    }

    if obj.contains_key("start") || obj.contains_key("end") {
        return Some("Period");
    }

    if (obj.contains_key("code") && obj.contains_key("system"))
        || (obj.contains_key("code") && obj.contains_key("display"))
    {
        return Some("Coding");
    }

    if obj.contains_key("value") && !obj.contains_key("code") {
        return Some("Identifier");
    }

    if obj.contains_key("reference")
        || (obj.contains_key("identifier") && obj.contains_key("type"))
        || obj.contains_key("display")
    {
        return Some("Reference");
    }

    None
}
