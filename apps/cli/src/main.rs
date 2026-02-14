use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as AnyhowContext, Result};
use clap::{ArgAction, Parser, Subcommand};
use zunder_codegen::generators::GeneratorConfig;
use serde_json::{Map, Value};
use zunder_context::{DefaultFhirContext, FhirContext};
use zunder_models::{Snapshot, StructureDefinition};
use zunder_registry_client::RegistryClient;
use zunder_snapshot::{
    generate_deep_snapshot, generate_structure_definition_differential,
    generate_structure_definition_snapshot, SnapshotExpander,
};
use zunder_fhirpath::value::{Collection, ValueData};
use zunder_fhirpath::vm::Plan;
use zunder_fhirpath::{Context, Engine, Value as FhirValue};

#[derive(Parser)]
#[command(
    name = "tlq",
    about = "Command line interface for the tlq platform",
    version,
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Evaluate a FHIRPath expression against a resource (JSON) or empty context.
    FP {
        /// FHIRPath expression to evaluate.
        expr: String,
        /// Path to a resource JSON file (or "-" for stdin). Omit to evaluate against an empty context.
        resource: Option<PathBuf>,
        /// FHIR version (R4, R4B, R5).
        #[arg(short = 'v', long, default_value = "R5")]
        fhir_version: String,
        /// Enable strict semantics (for semantic-invalid tests / strict path validation).
        #[arg(long, action = ArgAction::SetTrue)]
        strict: bool,
        /// Override the compilation base type (e.g. Patient). Defaults to resourceType, if present.
        #[arg(long)]
        base_type: Option<String>,
        /// Output format: json (default) or fhirpath (one item per line).
        #[arg(long, default_value = "json")]
        output: String,
        /// Pretty-print JSON output (only for --output json).
        #[arg(long, action = ArgAction::SetTrue)]
        pretty: bool,
    },

    /// Visualize FHIRPath compiler pipeline (AST, HIR, VM Plan).
    Visualize {
        /// FHIRPath expression to visualize.
        expr: String,
        /// Output format: ascii, mermaid, dot.
        #[arg(short, long, default_value = "ascii")]
        format: String,
        /// Show only specific stage: ast, hir, plan, or all.
        #[arg(short, long, default_value = "all")]
        stage: String,
        /// Output file path (stdout if omitted).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Snapshot-related commands.
    Snap {
        #[command(subcommand)]
        command: SnapCommands,
    },

    /// Differential-related commands.
    Diff {
        #[command(subcommand)]
        command: DiffCommands,
    },

    /// Generate strongly typed models from a FHIR context (core + optional packages).
    Codegen {
        /// Output directory for generated files.
        #[arg(short, long, value_name = "DIR", default_value = "generated")]
        output: PathBuf,
        /// FHIR version (R4, R4B, R5).
        #[arg(short = 'v', long, default_value = "R4")]
        fhir_version: String,
        /// Additional packages to load (format NAME#VERSION). Repeatable.
        #[arg(short = 'p', long = "package", value_name = "NAME#VERSION")]
        packages: Vec<String>,
        /// Generate documentation comments.
        #[arg(long, default_value_t = true, action = ArgAction::Set)]
        docs: bool,
        /// Generate serde derive/attributes.
        #[arg(long, default_value_t = true, action = ArgAction::Set)]
        serde: bool,
        /// Optional module path prefix for generated modules.
        #[arg(long)]
        module_prefix: Option<String>,
    },

    /// Generate FHIR type metadata for the format crate (array cardinality info).
    GenFormatMetadata {
        /// Output file path for the generated JSON metadata.
        #[arg(short, long, default_value = "libs/fhir-format/src/fhir_type_metadata.json")]
        output: PathBuf,
        /// FHIR version (R4, R4B, R5).
        #[arg(short = 'v', long, default_value = "R4")]
        fhir_version: String,
    },

    /// Print CLI version.
    Version,
}

#[derive(Subcommand)]
enum SnapCommands {
    /// Generate a snapshot from base + differential StructureDefinitions.
    Gen {
        /// Path to the base StructureDefinition JSON file (optional).
        #[arg(short, long)]
        base: Option<PathBuf>,
        /// Path to the derived StructureDefinition JSON file (with differential).
        #[arg(short, long)]
        differential: PathBuf,
        /// Output file path (stdout if omitted).
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Pretty-print JSON output.
        #[arg(short, long)]
        pretty: bool,
        /// FHIR version (R4, R4B, R5).
        #[arg(short = 'v', long, default_value = "R4")]
        fhir_version: String,
        /// Additional packages to load (format NAME#VERSION). Repeatable.
        #[arg(short = 'p', long = "package", value_name = "NAME#VERSION")]
        packages: Vec<String>,
    },

    /// Expand a StructureDefinition snapshot (deep expansion).
    Expand {
        /// Path to the StructureDefinition JSON file (with snapshot).
        #[arg(short, long)]
        snapshot: PathBuf,
        /// Output file path (stdout if omitted).
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Pretty-print JSON output.
        #[arg(short, long)]
        pretty: bool,
        /// FHIR version (R4, R4B, R5).
        #[arg(short = 'v', long, default_value = "R4")]
        fhir_version: String,
        /// Additional packages to load (format NAME#VERSION). Repeatable.
        #[arg(short = 'p', long = "package", value_name = "NAME#VERSION")]
        packages: Vec<String>,
    },
}

#[derive(Subcommand)]
enum DiffCommands {
    /// Generate a differential by comparing derived vs base StructureDefinitions.
    Gen {
        /// Path to the base StructureDefinition JSON file.
        #[arg(short, long)]
        base: PathBuf,
        /// Path to the StructureDefinition JSON file with snapshot.
        #[arg(short, long)]
        snapshot: PathBuf,
        /// Output file path (stdout if omitted).
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Pretty-print JSON output.
        #[arg(short, long)]
        pretty: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
        }
        Commands::FP {
            expr,
            resource,
            fhir_version,
            strict,
            base_type,
            output,
            pretty,
        } => {
            run_fhirpath(
                &expr,
                resource.as_deref(),
                &fhir_version,
                strict,
                base_type.as_deref(),
                &output,
                pretty,
            )
            .await?;
        }
        Commands::Visualize {
            expr,
            format,
            stage,
            output,
        } => {
            run_visualize(&expr, &format, &stage, output.as_deref()).await?;
        }
        Commands::Snap {
            command:
                SnapCommands::Gen {
                    base,
                    differential,
                    output,
                    pretty,
                    fhir_version,
                    packages,
                },
        } => {
            let ctx = create_context(&fhir_version, &packages).await?;
            run_snapshot_gen(
                base.as_deref(),
                &differential,
                output.as_deref(),
                pretty,
                &ctx,
            )?;
        }
        Commands::Snap {
            command:
                SnapCommands::Expand {
                    snapshot,
                    output,
                    pretty,
                    fhir_version,
                    packages,
                },
        } => {
            let ctx = create_context(&fhir_version, &packages).await?;
            run_snapshot_expand(&snapshot, output.as_deref(), pretty, &ctx)?;
        }
        Commands::Diff {
            command:
                DiffCommands::Gen {
                    base,
                    snapshot,
                    output,
                    pretty,
                },
        } => {
            run_diff_gen(&base, &snapshot, output.as_deref(), pretty)?;
        }
        Commands::GenFormatMetadata {
            output,
            fhir_version,
        } => {
            run_gen_format_metadata(&output, &fhir_version).await?;
        }
        Commands::Codegen {
            output,
            fhir_version,
            packages,
            docs,
            serde,
            module_prefix,
        } => {
            run_codegen(
                &output,
                &fhir_version,
                &packages,
                docs,
                serde,
                module_prefix,
            )
            .await?;
        }
    }

    Ok(())
}

async fn run_fhirpath(
    expr: &str,
    resource_path: Option<&Path>,
    fhir_version: &str,
    strict: bool,
    base_type_override: Option<&str>,
    output: &str,
    pretty: bool,
) -> Result<()> {
    let json = match resource_path {
        None => None,
        Some(path) if path.to_string_lossy() == "-" => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("Failed to read JSON resource from stdin")?;
            let json: serde_json::Value =
                serde_json::from_str(&buf).context("stdin resource is not valid JSON")?;
            Some(json)
        }
        Some(path) => {
            let contents = fs::read_to_string(path)
                .with_context(|| format!("Failed to read resource file '{}'", path.display()))?;
            let json: serde_json::Value = serde_json::from_str(&contents)
                .with_context(|| format!("Resource file is not valid JSON: {}", path.display()))?;
            Some(json)
        }
    };

    let resource_value = json
        .as_ref()
        .map(|v| FhirValue::from_json(v.clone()))
        .unwrap_or_else(FhirValue::empty);

    let mut ctx = Context::new(resource_value);
    if strict {
        ctx = ctx.with_strict_semantics();
    }

    // Use explicit base type if provided, else infer from resourceType.
    let inferred_base_type = json
        .as_ref()
        .and_then(|j| j.get("resourceType"))
        .and_then(|rt| rt.as_str());

    // Base type controls compile-time path validation/type inference. In lenient mode, we avoid
    // passing a base type so JSON choice expansions like `valueQuantity` can still be evaluated
    // (matching the HL7 lenient/polymorphics behavior).
    let requested_base_type = if let Some(bt) = base_type_override {
        Some(bt)
    } else if strict {
        inferred_base_type
    } else {
        None
    };

    let base_type = if let Some(bt) = requested_base_type {
        // If the expression is already rooted with the type name (e.g. `Observation.value`),
        // don't pass a base type to the compiler, otherwise strict path validation would
        // reject the leading type segment.
        let trimmed = expr.trim_start();
        let rooted =
            trimmed.starts_with(bt) && trimmed.chars().nth(bt.len()).is_some_and(|c| c == '.');
        let rooted_fhir = trimmed.starts_with("FHIR.")
            && trimmed[4..].starts_with(bt)
            && trimmed.chars().nth(4 + bt.len()).is_some_and(|c| c == '.');
        if rooted || rooted_fhir {
            None
        } else {
            Some(bt)
        }
    } else {
        None
    };

    let engine = Engine::with_fhir_version(fhir_version)
        .await
        .with_context(|| {
            format!(
                "Failed to create FHIRPath engine for version {}",
                fhir_version
            )
        })?;

    let result = engine
        .evaluate_expr(expr, &ctx, base_type)
        .with_context(|| format!("Failed to evaluate expression: {}", expr))?;

    match output.to_ascii_lowercase().as_str() {
        "json" => {
            let stringify_plan = engine.compile("$this.toString()", None).ok();
            let json_out = collection_to_json(&engine, stringify_plan.as_ref(), &result);
            if pretty {
                println!("{}", serde_json::to_string_pretty(&json_out)?);
            } else {
                println!("{}", serde_json::to_string(&json_out)?);
            }
        }
        "fhirpath" | "lines" => {
            let stringify_plan = engine.compile("$this.toString()", None).ok();
            for item in result.iter() {
                if let Some(plan) = stringify_plan.as_ref() {
                    let (engine, plan) = (&engine, plan);
                    if let Some(s) = stringify_value(engine, plan, item) {
                        println!("{}", s);
                        continue;
                    }
                }
                println!("{:?}", item);
            }
        }
        other => anyhow::bail!(
            "Unsupported output format: {} (use json or fhirpath)",
            other
        ),
    }

    Ok(())
}

fn run_snapshot_gen(
    base: Option<&Path>,
    differential: &Path,
    output: Option<&Path>,
    pretty: bool,
    context: &dyn FhirContext,
) -> Result<()> {
    let base_sd = match base {
        Some(path) => {
            let base_sd_json = load_structure_definition(path)?;
            Some(structure_definition_from_value(&base_sd_json)?)
        }
        None => None,
    };

    let diff_sd_json = load_structure_definition(differential)?;
    let diff_sd = structure_definition_from_value(&diff_sd_json)?;

    let result_sd = generate_structure_definition_snapshot(base_sd.as_ref(), &diff_sd, context)
        .with_context(|| "Failed to generate snapshot from StructureDefinitions".to_string())?;

    let result_value = serde_json::to_value(&result_sd)?;
    write_json_output(&result_value, output, pretty)?;
    Ok(())
}

fn run_snapshot_expand(
    snapshot: &Path,
    output: Option<&Path>,
    pretty: bool,
    context: &dyn FhirContext,
) -> Result<()> {
    let sd_json = load_structure_definition(snapshot)?;
    let sd_typed = structure_definition_from_value(&sd_json)?;
    let snapshot = sd_typed
        .snapshot
        .as_ref()
        .with_context(|| "StructureDefinition missing snapshot field".to_string())?;

    let expander = SnapshotExpander::new();
    let expanded_elements = expander
        .expand_snapshot(snapshot, context)
        .with_context(|| "Failed to expand snapshot".to_string())?;

    let mut result_sd = sd_json.clone();
    result_sd["snapshot"] = serde_json::to_value(&Snapshot {
        element: expanded_elements,
    })?;

    // Also provide a deep snapshot as a convenience
    if let Ok(deep_snapshot) = generate_deep_snapshot(snapshot, context) {
        result_sd["deepSnapshot"] = serde_json::to_value(&deep_snapshot)?;
    }

    write_json_output(&result_sd, output, pretty)?;
    Ok(())
}

fn run_diff_gen(base: &Path, snapshot: &Path, output: Option<&Path>, pretty: bool) -> Result<()> {
    let base_sd_json = load_structure_definition(base)?;
    let snap_sd_json = load_structure_definition(snapshot)?;
    let base_sd = structure_definition_from_value(&base_sd_json)?;
    let snap_sd = structure_definition_from_value(&snap_sd_json)?;

    let result_sd = generate_structure_definition_differential(&base_sd, &snap_sd)
        .with_context(|| "Failed to generate differential from StructureDefinitions".to_string())?;

    let result_value = serde_json::to_value(&result_sd)?;
    write_json_output(&result_value, output, pretty)?;
    Ok(())
}

async fn run_gen_format_metadata(output: &Path, fhir_version: &str) -> Result<()> {
    use std::collections::BTreeMap;

    let ctx = create_context(fhir_version, &[]).await?;
    let all_sds = ctx.all_structure_definitions();

    // Outer map: type_name -> { property_name -> { type, multiple } }
    let mut metadata: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();

    for sd_value in &all_sds {
        let sd: StructureDefinition = match serde_json::from_value(sd_value.as_ref().clone()) {
            Ok(sd) => sd,
            Err(_) => continue,
        };

        let snapshot = match &sd.snapshot {
            Some(s) => s,
            None => continue,
        };

        for element in &snapshot.element {
            let path = &element.path;

            // Split path into parent_type and property_name.
            // E.g., "Patient.name" -> ("Patient", "name")
            // E.g., "Patient.contact.name" -> ("Patient.contact", "name")
            // Skip root elements like "Patient" (no dot).
            let dot_pos = match path.rfind('.') {
                Some(p) => p,
                None => continue,
            };

            let parent_type = &path[..dot_pos];
            let property_name = &path[dot_pos + 1..];

            // Skip sliced elements (contain ':') — they don't define new properties.
            if property_name.contains(':') {
                continue;
            }

            // Determine if this is an array: max != "0" && max != "1"
            let is_multiple = element
                .max
                .as_ref()
                .map(|m| m != "0" && m != "1")
                .unwrap_or(false);

            // Determine the element type.
            let element_type = if let Some(types) = &element.types {
                if let Some(first) = types.first() {
                    if first.code == "BackboneElement" || first.code == "Element" {
                        // BackboneElement: use the full path as the synthetic type name.
                        path.to_string()
                    } else {
                        first.code.clone()
                    }
                } else {
                    "string".to_string()
                }
            } else if element.content_reference.is_some() {
                // Content references point to another element's structure.
                "BackboneElement".to_string()
            } else {
                "string".to_string()
            };

            let type_entry = metadata
                .entry(parent_type.to_string())
                .or_default();

            let prop_value = serde_json::json!({
                "type": element_type,
                "multiple": is_multiple
            });

            type_entry.insert(property_name.to_string(), prop_value);
        }
    }

    let json = serde_json::to_string_pretty(&metadata)?;
    fs::write(output, &json)
        .with_context(|| format!("Failed to write metadata to {:?}", output))?;

    eprintln!(
        "Generated format metadata with {} types to {:?}",
        metadata.len(),
        output
    );
    Ok(())
}

async fn run_codegen(
    output: &Path,
    fhir_version: &str,
    packages: &[String],
    docs: bool,
    serde: bool,
    module_prefix: Option<String>,
) -> Result<()> {
    let context = create_context(fhir_version, packages).await?;

    let config = GeneratorConfig {
        generate_docs: docs,
        generate_serde: serde,
        module_prefix,
    };

    let generated = zunder_codegen::generate_rust_from_context(&context, output, config)
        .with_context(|| "Failed to generate Rust models from context".to_string())?;

    println!(
        "Generated {} Rust modules into {}",
        generated,
        output.display()
    );

    Ok(())
}

async fn create_context(
    fhir_version: &str,
    extra_packages: &[String],
) -> Result<DefaultFhirContext> {
    let registry = Arc::new(RegistryClient::new(None));

    let (core_name, core_version) = match fhir_version {
        "R4" => ("hl7.fhir.r4.core", "4.0.1"),
        "R4B" => ("hl7.fhir.r4b.core", "4.3.0"),
        "R5" => ("hl7.fhir.r5.core", "5.0.0"),
        other => anyhow::bail!("Unsupported FHIR version: {} (use R4, R4B, or R5)", other),
    };

    // Load core package (with dependencies)
    let mut combined_packages = registry
        .load_package_with_dependencies(core_name, Some(core_version))
        .await
        .with_context(|| format!("Failed to load core package {}#{}", core_name, core_version))?;

    // Load additional packages (with dependencies) and merge
    for pkg in extra_packages {
        let (name, version) = parse_name_version(pkg)?;
        let mut deps = registry
            .load_package_with_dependencies(&name, Some(&version))
            .await
            .with_context(|| format!("Failed to load package {}#{}", name, version))?;
        combined_packages.append(&mut deps);
    }

    // Deduplicate packages by name#version to avoid double-including shared deps
    let mut seen = std::collections::HashSet::new();
    combined_packages
        .retain(|pkg| seen.insert(format!("{}#{}", pkg.manifest.name, pkg.manifest.version)));

    Ok(DefaultFhirContext::from_packages(combined_packages))
}

fn parse_name_version(s: &str) -> Result<(String, String)> {
    let (name, version) = s
        .split_once('#')
        .context("Package must be in format name#version")?;
    if name.trim().is_empty() || version.trim().is_empty() {
        anyhow::bail!("Package must be in format name#version (non-empty)");
    }
    Ok((name.trim().to_string(), version.trim().to_string()))
}

fn load_structure_definition(path: &Path) -> Result<Value> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {:?}", path.display()))?;

    let value: Value = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse JSON in {:?}", path.display()))?;

    if value.get("resourceType").and_then(|v| v.as_str()) != Some("StructureDefinition") {
        anyhow::bail!(
            "File {:?} is not a StructureDefinition (missing or incorrect resourceType)",
            path
        );
    }

    if value.get("url").is_none() {
        anyhow::bail!(
            "StructureDefinition in {:?} missing required field: url",
            path
        );
    }

    Ok(value)
}

fn structure_definition_from_value(value: &Value) -> Result<StructureDefinition> {
    serde_json::from_value(value.clone())
        .with_context(|| "Failed to deserialize StructureDefinition into typed model".to_string())
}

fn write_json_output(value: &Value, output: Option<&Path>, pretty: bool) -> Result<()> {
    if let Some(output_path) = output {
        let content = if pretty {
            serde_json::to_string_pretty(value)?
        } else {
            serde_json::to_string(value)?
        };
        fs::write(output_path, content)
            .with_context(|| format!("Failed to write to {:?}", output_path))?;
        eprintln!("✓ Wrote output to {:?}", output_path);
    } else if pretty {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        println!("{}", serde_json::to_string(value)?);
    }

    Ok(())
}

fn stringify_value(engine: &Engine, plan: &Arc<Plan>, value: &FhirValue) -> Option<String> {
    let ctx = Context::new(FhirValue::empty()).push_this(value.clone());
    engine
        .evaluate(plan, &ctx)
        .ok()
        .and_then(|c| c.as_string().ok())
        .map(|s| s.to_string())
}

fn value_to_json(engine: &Engine, stringify_plan: Option<&Arc<Plan>>, value: &FhirValue) -> Value {
    match value.data() {
        ValueData::Boolean(b) => Value::Bool(*b),
        ValueData::Integer(i) => Value::Number((*i).into()),
        ValueData::Decimal(d) => Value::String(d.to_string()),
        ValueData::String(s) => Value::String(s.to_string()),
        ValueData::Date { .. } | ValueData::DateTime { .. } | ValueData::Time { .. } => {
            if let Some(plan) = stringify_plan {
                if let Some(s) = stringify_value(engine, plan, value) {
                    return Value::String(s);
                }
            }
            Value::String(format!("{:?}", value))
        }
        ValueData::Quantity { value, unit } => Value::Object({
            let mut map = Map::new();
            map.insert("value".to_string(), Value::String(value.to_string()));
            map.insert("unit".to_string(), Value::String(unit.to_string()));
            map
        }),
        ValueData::Object(map) => {
            let mut obj = Map::new();
            for (k, coll) in map.iter() {
                obj.insert(
                    k.to_string(),
                    collection_to_json(engine, stringify_plan, coll),
                );
            }
            Value::Object(obj)
        }
        ValueData::LazyJson { .. } => {
            // Materialize lazy JSON first and recursively convert
            let materialized = value.materialize();
            value_to_json(engine, stringify_plan, &materialized)
        }
        ValueData::Empty => Value::Null,
    }
}

fn collection_to_json(
    engine: &Engine,
    stringify_plan: Option<&Arc<Plan>>,
    coll: &Collection,
) -> Value {
    let items: Vec<Value> = coll
        .iter()
        .map(|v| value_to_json(engine, stringify_plan, v))
        .collect();
    Value::Array(items)
}

async fn run_visualize(expr: &str, format: &str, stage: &str, output: Option<&Path>) -> Result<()> {
    use zunder_fhirpath::{PipelineVisualization, VisualizationFormat};

    // Parse format
    let viz_format = match format.to_lowercase().as_str() {
        "ascii" | "tree" => VisualizationFormat::AsciiTree,
        "mermaid" | "md" => VisualizationFormat::Mermaid,
        "dot" | "graphviz" => VisualizationFormat::Dot,
        _ => anyhow::bail!("Invalid format '{}'. Use: ascii, mermaid, or dot", format),
    };

    // Create engine (use empty context for visualization - no FHIR validation needed)
    let engine = Engine::with_fhir_version("R5").await?;

    // Generate visualization
    let result = match stage.to_lowercase().as_str() {
        "all" => {
            let viz: PipelineVisualization = engine
                .visualize_pipeline(expr, viz_format)
                .with_context(|| format!("Failed to visualize expression: {}", expr))?;

            format!(
                "FHIRPath Expression: {}\n{}\n\n\
                 📊 ABSTRACT SYNTAX TREE (AST)\n{}\n{}\n\n\
                 🔧 HIGH-LEVEL IR (HIR - with types)\n{}\n{}\n\n\
                 ⚙️  VM BYTECODE PLAN\n{}\n{}",
                expr,
                "=".repeat(80),
                "─".repeat(80),
                viz.ast,
                "─".repeat(80),
                viz.hir,
                "─".repeat(80),
                viz.plan
            )
        }
        "ast" => {
            let ast = engine
                .visualize_ast(expr, viz_format)
                .with_context(|| format!("Failed to visualize AST: {}", expr))?;
            format!("AST for: {}\n{}\n{}", expr, "─".repeat(80), ast)
        }
        "hir" => {
            let hir = engine
                .visualize_hir(expr, viz_format)
                .with_context(|| format!("Failed to visualize HIR: {}", expr))?;
            format!("HIR for: {}\n{}\n{}", expr, "─".repeat(80), hir)
        }
        "plan" | "vm" => {
            let plan = engine
                .visualize_plan(expr, viz_format)
                .with_context(|| format!("Failed to visualize VM Plan: {}", expr))?;
            format!("VM Plan for: {}\n{}\n{}", expr, "─".repeat(80), plan)
        }
        _ => anyhow::bail!("Invalid stage '{}'. Use: all, ast, hir, or plan", stage),
    };

    // Write output
    if let Some(output_path) = output {
        fs::write(output_path, &result)
            .with_context(|| format!("Failed to write to {:?}", output_path))?;
        eprintln!("✓ Wrote visualization to {:?}", output_path);
    } else {
        println!("{}", result);
    }

    Ok(())
}
