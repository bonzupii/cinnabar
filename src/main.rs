mod ast;
mod lexer;
mod parser;
mod resolver;

use ariadne::Color;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use ariadne::Source;
use clap::Parser as ClapParser;
use lexer::Lexer;
use parser::Parser;
use resolver::Resolver;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(ClapParser)]
#[command(name = "cinnabar", version, about = "The Cinnabar compiler")]
struct Cli {
    /// Input Cinnabar source file
    input: PathBuf,

    /// Print the parsed AST
    #[arg(long)]
    dump_ast: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let file_id = cli.input.display().to_string();

    let source = match fs::read_to_string(&cli.input) {
        Ok(content) => content,
        Err(io_err) => {
            eprintln!("Failed to read file '{}': {}", file_id, io_err);
            return ExitCode::FAILURE;
        }
    };

    let mut lexer = Lexer::new(&source);
    let tokens = match lexer.tokenize() {
        Ok(token_list) => token_list,
        Err(lex_err) => {
            let span_range = lex_err.span.start..lex_err.span.end;
            let report_res = Report::build(ReportKind::Error, (file_id.as_str(), span_range.clone()))
                .with_message(lex_err.message)
                .with_label(
                    Label::new((file_id.as_str(), span_range))
                        .with_message("Lexical error")
                        .with_color(Color::Red),
                )
                .finish()
                .print((file_id.as_str(), Source::from(&source)));

            if let Err(render_err) = report_res {
                eprintln!("Failed to render lexical error diagnostic: {}", render_err);
            }
            return ExitCode::FAILURE;
        }
    };

    let mut parser = Parser::new(&tokens);
    let ast = match parser.parse_program() {
        Ok(parsed_ast) => parsed_ast,
        Err(parse_err) => {
            let span_range = parse_err.span.start..parse_err.span.end;
            let report_res = Report::build(ReportKind::Error, (file_id.as_str(), span_range.clone()))
                .with_message(parse_err.message)
                .with_label(
                    Label::new((file_id.as_str(), span_range))
                        .with_message("Syntax error")
                        .with_color(Color::Red),
                )
                .finish()
                .print((file_id.as_str(), Source::from(&source)));

            if let Err(render_err) = report_res {
                eprintln!("Failed to render syntax error diagnostic: {}", render_err);
            }
            return ExitCode::FAILURE;
        }
    };

    let mut resolver = Resolver::new();
    match resolver.resolve_program(&ast) {
        Ok(()) => {
            let stdout = io::stdout();
            let mut handle = stdout.lock();

            let write_result = if cli.dump_ast {
                writeln!(handle, "{:#?}", ast)
            } else {
                writeln!(handle, "Successfully parsed and resolved {} top-level items.", ast.len())
            };

            if let Err(io_err) = write_result {
                if io_err.kind() == io::ErrorKind::BrokenPipe {
                    return ExitCode::SUCCESS;
                }
                eprintln!("Failed to write output: {}", io_err);
                return ExitCode::FAILURE;
            }

            ExitCode::SUCCESS
        }
        Err(resolve_errors) => {
            let mut err_idx = 0;
            while err_idx < resolve_errors.len() {
                let resolve_err = &resolve_errors[err_idx];
                let span_range = resolve_err.span.start..resolve_err.span.end;
                let report_res = Report::build(ReportKind::Error, (file_id.as_str(), span_range.clone()))
                    .with_message(resolve_err.message.clone())
                    .with_label(
                        Label::new((file_id.as_str(), span_range))
                            .with_message("Resolution error")
                            .with_color(Color::Red),
                    )
                    .finish()
                    .print((file_id.as_str(), Source::from(&source)));

                if let Err(render_err) = report_res {
                    eprintln!("Failed to render resolution error diagnostic: {}", render_err);
                }
                err_idx += 1;
            }
            ExitCode::FAILURE
        }
    }
}
