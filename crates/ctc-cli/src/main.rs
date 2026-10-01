#![forbid(unsafe_code)]

use std::{
    env,
    io::{self, IsTerminal},
    path::PathBuf,
    process::ExitCode,
    sync::Arc,
};

use clap::{Parser, Subcommand, ValueEnum, error::ErrorKind};
use ctc_core::{
    AdHocCheckOptions, CheckOptions, CoverageOptions, Diagnostic, DiagnosticCategory, Engine,
    ExplainFailure, ExplainNode, ExplainOptions, ExplainReport, ExplainStep, ExplainStepKind,
    ExplainTarget, GuardOptions, LanguageRegistry, MatchMode, RunReport, SearchScope,
    diagnostic::TextRange,
};
use ctc_language_cpp::CppAdapter;
use ctc_language_rust::RustAdapter;
use ctc_language_typescript::TypeScriptAdapter;

#[derive(Debug, Parser)]
#[command(
    name = "ctc",
    version,
    about = "Check source files against code-shaped templates"
)]
struct Cli {
    #[command(flatten)]
    check: CheckArgs,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(hide = true)]
    Check(CheckArgs),

    /// Validate one template without matching source files.
    ValidateTemplate(ValidateTemplateArgs),

    /// Show how one rule or template matches one source file.
    Explain(ExplainArgs),

    /// List source files in the watched area that no rule selects.
    Coverage(CoverageArgs),

    /// Report changes that make the rules weaker than at a Git reference.
    Guard(GuardArgs),
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum OutputFormat {
    #[default]
    Human,
    Json,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ModeArgument {
    Exact,
    Contains,
    Forbid,
    Every,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ScopeArgument {
    TopLevel,
    Descendants,
    FunctionBody,
    ClassBody,
}

impl From<ModeArgument> for MatchMode {
    fn from(value: ModeArgument) -> Self {
        match value {
            ModeArgument::Exact => Self::Exact,
            ModeArgument::Contains => Self::Contains,
            ModeArgument::Forbid => Self::Forbid,
            ModeArgument::Every => Self::Every,
        }
    }
}

impl From<ScopeArgument> for SearchScope {
    fn from(value: ScopeArgument) -> Self {
        match value {
            ScopeArgument::TopLevel => Self::TopLevel,
            ScopeArgument::Descendants => Self::Descendants,
            ScopeArgument::FunctionBody => Self::FunctionBody,
            ScopeArgument::ClassBody => Self::ClassBody,
        }
    }
}

#[derive(Debug, clap::Args)]
struct CheckArgs {
    /// Files or directories that limit the selected source set.
    paths: Vec<PathBuf>,

    /// Root directory for template discovery.
    #[arg(long)]
    root: Option<PathBuf>,

    /// Configuration file relative to the project root.
    #[arg(long)]
    config: Option<PathBuf>,

    /// Run only the named configured rule. Repeat for more rules.
    #[arg(long)]
    rule: Vec<String>,

    /// Use one ad hoc template instead of discovered rules.
    #[arg(long)]
    template: Option<PathBuf>,

    /// Override the mode of an ad hoc template.
    #[arg(long)]
    mode: Option<ModeArgument>,

    /// Select the syntax region searched by an ad hoc template.
    #[arg(long, value_enum)]
    scope: Option<ScopeArgument>,

    /// Show this message for each rule failure of an ad hoc template.
    #[arg(long)]
    message: Option<String>,

    /// Select human or JSON diagnostics.
    #[arg(long, value_enum, default_value_t)]
    format: OutputFormat,

    /// Control colors in human output.
    #[arg(long, value_enum, default_value_t)]
    color: ColorChoice,
}

#[derive(Debug, clap::Args)]
struct ValidateTemplateArgs {
    /// Template file to validate.
    path: PathBuf,

    /// Select human or JSON diagnostics.
    #[arg(long, value_enum, default_value_t)]
    format: OutputFormat,

    /// Control colors in human output.
    #[arg(long, value_enum, default_value_t)]
    color: ColorChoice,
}

#[derive(Debug, clap::Args)]
#[command(group(clap::ArgGroup::new("target").required(true).args(["rule", "template"])))]
struct ExplainArgs {
    /// Source file to explain.
    path: PathBuf,

    /// Explain the named configured template rule.
    #[arg(long, conflicts_with = "template")]
    rule: Option<String>,

    /// Explain one ad hoc template instead of a configured rule.
    #[arg(long, conflicts_with = "config")]
    template: Option<PathBuf>,

    /// Override the mode of an ad hoc template.
    #[arg(long, value_enum)]
    mode: Option<ModeArgument>,

    /// Select the syntax region searched by an ad hoc template.
    #[arg(long, value_enum)]
    scope: Option<ScopeArgument>,

    /// Root directory for template discovery.
    #[arg(long)]
    root: Option<PathBuf>,

    /// Configuration file relative to the project root.
    #[arg(long)]
    config: Option<PathBuf>,

    /// Select human or JSON output.
    #[arg(long, value_enum, default_value_t)]
    format: OutputFormat,

    /// Control colors in human output.
    #[arg(long, value_enum, default_value_t)]
    color: ColorChoice,
}

#[derive(Debug, clap::Args)]
struct CoverageArgs {
    /// Root directory for template discovery.
    #[arg(long)]
    root: Option<PathBuf>,

    /// Configuration file relative to the project root.
    #[arg(long)]
    config: Option<PathBuf>,

    /// Select human or JSON diagnostics.
    #[arg(long, value_enum, default_value_t)]
    format: OutputFormat,

    /// Control colors in human output.
    #[arg(long, value_enum, default_value_t)]
    color: ColorChoice,
}

#[derive(Debug, clap::Args)]
struct GuardArgs {
    /// Git reference to compare with, for example origin/main.
    #[arg(long)]
    base: String,

    /// Do not report changes to rule definitions and templates.
    #[arg(long)]
    allow_rule_changes: bool,

    /// Root directory for template discovery.
    #[arg(long)]
    root: Option<PathBuf>,

    /// Configuration file relative to the project root.
    #[arg(long)]
    config: Option<PathBuf>,

    /// Select human or JSON diagnostics.
    #[arg(long, value_enum, default_value_t)]
    format: OutputFormat,

    /// Control colors in human output.
    #[arg(long, value_enum, default_value_t)]
    color: ColorChoice,
}

fn main() -> ExitCode {
    let error_format = requested_output_format();
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            let report = RunReport::new(vec![Diagnostic::new(
                "CTC0001",
                DiagnosticCategory::InvalidCommandLine,
                error.to_string().trim().to_string(),
                2,
            )]);
            print_report(&report, error_format, requested_color_choice());
            return ExitCode::from(2);
        }
    };

    let mut languages = LanguageRegistry::new();
    languages.register(Arc::new(CppAdapter::new()));
    languages.register(Arc::new(RustAdapter::new()));
    languages.register(Arc::new(TypeScriptAdapter::new()));
    let engine = Engine::new(languages);

    let (report, format, color) = match cli.command {
        None => {
            let format = cli.check.format;
            let color = cli.check.color;
            (run_check(&engine, cli.check), format, color)
        }
        Some(Command::Check(arguments)) => {
            let format = arguments.format;
            let color = arguments.color;
            (run_check(&engine, arguments), format, color)
        }
        Some(Command::ValidateTemplate(arguments)) => {
            let format = arguments.format;
            let color = arguments.color;
            (engine.validate_template(&arguments.path), format, color)
        }
        Some(Command::Explain(arguments)) => return run_explain(&engine, arguments),
        Some(Command::Coverage(arguments)) => {
            let report = engine.coverage(&CoverageOptions {
                root: arguments.root.unwrap_or_else(current_directory),
                config_path: arguments.config,
            });
            (report, arguments.format, arguments.color)
        }
        Some(Command::Guard(arguments)) => {
            let report = engine.guard(&GuardOptions {
                root: arguments.root.unwrap_or_else(current_directory),
                config_path: arguments.config,
                base_ref: arguments.base,
                allow_rule_changes: arguments.allow_rule_changes,
            });
            (report, arguments.format, arguments.color)
        }
    };
    print_report(&report, format, color);
    ExitCode::from(report.exit_code())
}

fn current_directory() -> PathBuf {
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn requested_output_format() -> OutputFormat {
    let arguments = env::args_os().collect::<Vec<_>>();
    for (index, argument) in arguments.iter().enumerate() {
        let value = argument.to_string_lossy();
        if value == "--format"
            && arguments
                .get(index + 1)
                .is_some_and(|next| next.to_string_lossy() == "json")
        {
            return OutputFormat::Json;
        }
        if value == "--format=json" {
            return OutputFormat::Json;
        }
    }
    OutputFormat::Human
}

fn requested_color_choice() -> ColorChoice {
    let arguments = env::args_os().collect::<Vec<_>>();
    for (index, argument) in arguments.iter().enumerate() {
        let value = argument.to_string_lossy();
        if value == "--color"
            && let Some(next) = arguments.get(index + 1)
        {
            return parse_color_choice(&next.to_string_lossy());
        }
        if let Some(value) = value.strip_prefix("--color=") {
            return parse_color_choice(value);
        }
    }
    ColorChoice::Auto
}

fn parse_color_choice(value: &str) -> ColorChoice {
    match value {
        "always" => ColorChoice::Always,
        "never" => ColorChoice::Never,
        _ => ColorChoice::Auto,
    }
}

fn run_check(engine: &Engine, arguments: CheckArgs) -> RunReport {
    if arguments.template.is_some() && !arguments.rule.is_empty() {
        return invalid_command("`--template` and `--rule` cannot occur together.");
    }
    if arguments.template.is_some() && arguments.config.is_some() {
        return invalid_command("`--template` and `--config` cannot occur together.");
    }
    if arguments.mode.is_some() && arguments.template.is_none() {
        return invalid_command("`--mode` requires `--template`.");
    }
    if arguments.scope.is_some() && arguments.template.is_none() {
        return invalid_command("`--scope` requires `--template`.");
    }
    if arguments.message.is_some() && arguments.template.is_none() {
        return invalid_command("`--message` requires `--template`.");
    }
    let root = arguments
        .root
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    if let Some(template_path) = arguments.template {
        engine.check_ad_hoc(&AdHocCheckOptions {
            root,
            paths: arguments.paths,
            template_path,
            mode_override: arguments.mode.map(Into::into),
            scope: arguments
                .scope
                .map(Into::into)
                .unwrap_or(SearchScope::TopLevel),
            message: arguments.message,
        })
    } else {
        engine.check(&CheckOptions {
            root,
            config_path: arguments.config,
            paths: arguments.paths,
            rule_ids: arguments.rule,
        })
    }
}

fn run_explain(engine: &Engine, arguments: ExplainArgs) -> ExitCode {
    let misuse = if arguments.template.is_none() && arguments.mode.is_some() {
        Some("`--mode` requires `--template`.")
    } else if arguments.template.is_none() && arguments.scope.is_some() {
        Some("`--scope` requires `--template`.")
    } else {
        None
    };
    if let Some(message) = misuse {
        print_report(&invalid_command(message), arguments.format, arguments.color);
        return ExitCode::from(2);
    }
    let target = match (arguments.rule, arguments.template) {
        (_, Some(path)) => ExplainTarget::Template {
            path,
            mode_override: arguments.mode.map(Into::into),
            scope: arguments
                .scope
                .map(Into::into)
                .unwrap_or(SearchScope::TopLevel),
        },
        (Some(rule), None) => ExplainTarget::Rule(rule),
        (None, None) => {
            let report = invalid_command("`explain` needs `--rule` or `--template`.");
            print_report(&report, arguments.format, arguments.color);
            return ExitCode::from(2);
        }
    };
    let options = ExplainOptions {
        root: arguments
            .root
            .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
        config_path: arguments.config,
        source_path: arguments.path,
        target,
    };
    match engine.explain(&options) {
        Ok(report) => {
            match arguments.format {
                OutputFormat::Json => match serde_json::to_string_pretty(&report) {
                    Ok(json) => println!("{json}"),
                    Err(error) => eprintln!("ctc: cannot serialize the JSON report: {error}"),
                },
                OutputFormat::Human => print_explanation(&report, Colors::new(arguments.color)),
            }
            ExitCode::from(report.exit_code())
        }
        Err(report) => {
            print_report(&report, arguments.format, arguments.color);
            ExitCode::from(report.exit_code())
        }
    }
}

fn print_explanation(report: &ExplainReport, colors: Colors) {
    println!(
        "{}: {} ({}, {})",
        colors.bold("Rule"),
        colors.magenta(&report.rule_id),
        mode_name(report.mode),
        scope_name(report.scope)
    );
    println!("{}: {}", colors.bold("Template"), report.template);
    println!("{}: {}", colors.bold("Source"), report.source);
    let result = if report.matches {
        colors.green("matches")
    } else {
        let failed = report
            .candidates
            .iter()
            .filter(|candidate| !candidate.template_matched)
            .count();
        let found = report.candidates.len() - failed;
        let detail = match report.mode {
            MatchMode::Every => format!(
                " ({failed} of {} candidates failed)",
                report.candidates.len()
            ),
            MatchMode::Forbid => format!(" (the template was found {found} time(s))"),
            MatchMode::Exact | MatchMode::Contains => String::new(),
        };
        colors.red(&format!("does not match{detail}"))
    };
    println!("{}: {result}", colors.bold("Result"));
    if report.candidates.is_empty() {
        println!();
        println!("No candidate nodes were found.");
    }
    for (index, candidate) in report.candidates.iter().enumerate() {
        println!();
        let state = if candidate.template_matched {
            colors.green("template matches")
        } else {
            colors.red("template does not match")
        };
        println!(
            "Candidate {} at {}: {state}",
            index + 1,
            colors.cyan(&location(&candidate.source))
        );
        for step in &candidate.steps {
            print_step(step, colors);
        }
        if let Some(failure) = &candidate.failure {
            print_failure(failure, colors);
        }
    }
    let extra = report
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            !report.candidates.iter().any(|candidate| {
                candidate
                    .failure
                    .as_ref()
                    .is_some_and(|failure| same_diagnostic(&failure.diagnostic, diagnostic))
            })
        })
        .collect::<Vec<_>>();
    if !extra.is_empty() {
        println!();
        println!("{}:", colors.bold("Diagnostics not shown above"));
        print_report(
            &RunReport::new(extra.into_iter().cloned().collect()),
            OutputFormat::Human,
            if colors.enabled {
                ColorChoice::Always
            } else {
                ColorChoice::Never
            },
        );
    }
}

fn print_step(step: &ExplainStep, colors: Colors) {
    let indent = "  ".repeat(step.depth + 1);
    let template = format!(
        "{}:{}",
        step.template.start.line, step.template.start.column
    );
    let name = step.name.as_deref().unwrap_or_default();
    match step.kind {
        ExplainStepKind::Sequence => {
            let count = match step.nodes.len() {
                0 => "no nodes".to_string(),
                1 => "1 node".to_string(),
                count => format!("{count} nodes"),
            };
            println!(
                "{indent}{} {} -> {count}",
                colors.cyan(&template),
                colors.yellow(&format!("{{{{* {name} }}}}"))
            );
            for node in &step.nodes {
                println!("{indent}  {}", node_text(node, colors));
            }
        }
        ExplainStepKind::OptionalAbsent => {
            let label = if name.is_empty() {
                format!(
                    "optional {}",
                    step.template_kind.as_deref().unwrap_or_default()
                )
            } else {
                format!("{{{{? {name} }}}}")
            };
            println!(
                "{indent}{} {} -> absent",
                colors.cyan(&template),
                colors.yellow(&label)
            );
        }
        ExplainStepKind::Literal | ExplainStepKind::Capture | ExplainStepKind::Derived => {
            let label = match step.kind {
                ExplainStepKind::Literal => step.template_kind.clone().unwrap_or_default(),
                ExplainStepKind::Derived => format!("{{{{ {name} }}}} (derived)"),
                _ => format!("{{{{ {name} }}}}"),
            };
            let nodes = step
                .nodes
                .iter()
                .map(|node| node_text(node, colors))
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "{indent}{} {} -> {nodes}",
                colors.cyan(&template),
                colors.yellow(&label)
            );
        }
    }
}

fn node_text(node: &ExplainNode, colors: Colors) -> String {
    format!(
        "{} {} `{}`",
        colors.cyan(&format!(
            "{}:{}",
            node.range.start.line, node.range.start.column
        )),
        node.kind,
        node.text
    )
}

fn print_failure(failure: &ExplainFailure, colors: Colors) {
    let diagnostic = &failure.diagnostic;
    let at = diagnostic
        .source
        .as_ref()
        .or(diagnostic.template.as_ref())
        .map_or_else(|| "ctc".to_string(), location);
    println!(
        "  {} {}: {}",
        colors.bold("Stopped at"),
        colors.cyan(&at),
        colors.red(&diagnostic.message)
    );
    println!(
        "    {}: {}",
        colors.bold("Code"),
        colors.yellow(&diagnostic.code)
    );
    if let Some(expected) = &diagnostic.expected {
        println!(
            "    {}: {}",
            colors.bold("Expected"),
            colors.green(expected)
        );
    }
    if let Some(actual) = &diagnostic.actual {
        println!("    {}: {}", colors.bold("Found"), colors.red(actual));
    }
    if let Some(template) = &diagnostic.template {
        println!(
            "    {}: {}",
            colors.bold("Template"),
            colors.cyan(&location(template))
        );
    }
    if let Some(reason) = &failure.reason {
        println!("    {}: {reason}", colors.bold("Why"));
    }
}

fn location(range: &TextRange) -> String {
    format!("{}:{}:{}", range.path, range.start.line, range.start.column)
}

fn same_diagnostic(left: &Diagnostic, right: &Diagnostic) -> bool {
    left.code == right.code
        && left.message == right.message
        && left.source == right.source
        && left.template == right.template
}

fn mode_name(mode: MatchMode) -> &'static str {
    match mode {
        MatchMode::Exact => "exact",
        MatchMode::Contains => "contains",
        MatchMode::Forbid => "forbid",
        MatchMode::Every => "every",
    }
}

fn scope_name(scope: SearchScope) -> &'static str {
    match scope {
        SearchScope::TopLevel => "top-level",
        SearchScope::Descendants => "descendants",
        SearchScope::FunctionBody => "function-body",
        SearchScope::ClassBody => "class-body",
        SearchScope::Inside => "inside",
    }
}

fn invalid_command(message: &str) -> RunReport {
    RunReport::new(vec![Diagnostic::new(
        "CTC0001",
        DiagnosticCategory::InvalidCommandLine,
        message,
        2,
    )])
}

fn print_report(report: &RunReport, format: OutputFormat, color_choice: ColorChoice) {
    match format {
        OutputFormat::Json => match serde_json::to_string_pretty(report) {
            Ok(json) => println!("{json}"),
            Err(error) => eprintln!("ctc: cannot serialize the JSON report: {error}"),
        },
        OutputFormat::Human => {
            let colors = Colors::new(color_choice);
            for diagnostic in &report.diagnostics {
                let primary = diagnostic.source.as_ref().or(diagnostic.template.as_ref());
                let location = primary.map_or_else(
                    || "ctc".to_string(),
                    |range| format!("{}:{}:{}", range.path, range.start.line, range.start.column),
                );
                let rule = diagnostic
                    .rule_id
                    .as_ref()
                    .map_or_else(String::new, |rule| {
                        format!(" {}", colors.magenta(&format!("[{rule}]")))
                    });
                let headline = diagnostic
                    .rule_message
                    .as_ref()
                    .unwrap_or(&diagnostic.message);
                println!("{}{rule} {}", colors.cyan(&location), colors.red(headline));
                if diagnostic.rule_message.is_some() {
                    println!("  {}: {}", colors.bold("Detail"), diagnostic.message);
                }
                println!(
                    "  {}: {}",
                    colors.bold("Code"),
                    colors.yellow(&diagnostic.code)
                );
                if let Some(expected) = &diagnostic.expected {
                    println!("  {}: {}", colors.bold("Expected"), colors.green(expected));
                }
                if let Some(actual) = &diagnostic.actual {
                    println!("  {}: {}", colors.bold("Found"), colors.red(actual));
                }
                if diagnostic.source.is_some()
                    && let Some(template) = &diagnostic.template
                {
                    println!(
                        "  {}: {}",
                        colors.bold("Template"),
                        colors.cyan(&format!(
                            "{}:{}:{}",
                            template.path, template.start.line, template.start.column
                        ))
                    );
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Colors {
    enabled: bool,
}

impl Colors {
    fn new(choice: ColorChoice) -> Self {
        let enabled = match choice {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => io::stdout().is_terminal() && env::var_os("NO_COLOR").is_none(),
        };
        Self { enabled }
    }

    fn bold(self, value: &str) -> String {
        self.paint("1", value)
    }

    fn red(self, value: &str) -> String {
        self.paint("31", value)
    }

    fn green(self, value: &str) -> String {
        self.paint("32", value)
    }

    fn yellow(self, value: &str) -> String {
        self.paint("33", value)
    }

    fn magenta(self, value: &str) -> String {
        self.paint("35", value)
    }

    fn cyan(self, value: &str) -> String {
        self.paint("36", value)
    }

    fn paint(self, code: &str, value: &str) -> String {
        if self.enabled {
            format!("\u{1b}[{code}m{value}\u{1b}[0m")
        } else {
            value.to_string()
        }
    }
}
