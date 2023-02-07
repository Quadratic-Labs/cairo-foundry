#[cfg(test)]
pub mod tests;

use cairo_rs::{
	hint_processor::builtin_hint_processor::builtin_hint_processor_definition::BuiltinHintProcessor,
	serde::deserialize_program::{deserialize_program_json, ProgramJson},
	types::{errors::program_errors, program::Program},
	vm::{
		errors::{cairo_run_errors::CairoRunError, vm_errors::VirtualMachineError},
		hook::Hooks,
	},
};
use clap::{Args, ValueHint};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fmt::Display, fs::File, io, io::BufWriter, path::PathBuf, sync::Arc, time::Instant};
use uuid::Uuid;

use self::run::run_tests_for_one_file;

use super::{list::path_is_valid_directory, CommandExecution};
use std::fs;
use thiserror::Error;

use crate::{
	cairo_run::cairo_run,
	compile::cache::{compile_and_list_entrypoints, get_cache},
	compile::{self, compile},
	hints::{
		output_buffer::{clear_buffer, get_buffer, init_buffer},
		processor::setup_hint_processor,
		EXPECT_REVERT_FLAG,
	},
	hooks,
	io::{
		compiled_programs::{list_test_entrypoints, ListTestEntrypointsError},
		test_files::{list_test_files, ListTestsFilesError},
	},
};

pub mod run;

/// Enum containing the possible errors that you may encounter in the ``Test`` module
#[derive(Error, Debug)]
// Todo: Maybe use anyhow at this level
#[allow(clippy::large_enum_variant)]
pub enum TestCommandError {
	#[error("Failed to list test entrypoints for file {0}: {1}")]
	ListEntrypoints(PathBuf, String),
	#[error("Failed to compile file {0}: {1}")]
	RunTest(String, PathBuf, String),
	#[error(transparent)]
	IO(#[from] io::Error),
	#[error(transparent)]
	JsonDeSerialization(#[from] serde_json::Error),
	#[error(transparent)]
	Compile(#[from] compile::Error),
	#[error(transparent)]
	Program(#[from] program_errors::ProgramError),
	#[error(transparent)]
	CairoRun(#[from] CairoRunError),
	#[error(transparent)]
	ListTestsFiles(#[from] ListTestsFilesError),
	#[error(transparent)]
	ListTestEntripoints(#[from] ListTestEntrypointsError),
}

/// Structure containing the path to a cairo directory.
/// Used to execute all the tests files contained in this directory
#[derive(Args, Debug)]
pub struct TestArgs {
	/// Path to a cairo directory
	#[clap(short, long, value_hint=ValueHint::DirPath, value_parser=path_is_valid_directory, default_value="./")]
	pub root: PathBuf,
	#[clap(short, long, default_value_t = 1000000)]
	pub max_steps: u64,
}

/// Execute command output
#[derive(Debug, Serialize, Default)]
pub struct TestOutput(String);

impl Display for TestOutput {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", &self.0)
	}
}

/// Create a new ``Hooks`` object, with the followings hooks:
/// - pre_step_instruction
/// - post_step_instruction
///
/// see [src/hooks.rs]
fn setup_hooks() -> Hooks {
	Hooks::new(
		Arc::new(hooks::pre_step_instruction),
		Arc::new(hooks::post_step_instruction),
	)
}

impl CommandExecution<TestOutput, TestCommandError> for TestArgs {
	fn exec(&self) -> Result<TestOutput, TestCommandError> {
		// Declare hints
		let mut hint_processor = setup_hint_processor();
		let hooks = setup_hooks();

		list_test_files(&self.root)?
			// .into_par_iter()
			.into_iter()
			.map(|op| get_cache(op))
			.filter_map(compile_and_list_entrypoints)
			.map(|(path_to_original, path_to_compiled, test_entrypoints)| {
				let file = fs::File::open(path_to_compiled).unwrap();
				let reader = io::BufReader::new(file);
				let program_json = deserialize_program_json(reader)?;

				run_tests_for_one_file(
					&mut hint_processor,
					path_to_original,
					program_json,
					test_entrypoints,
					hooks.clone(),
					self.max_steps,
				)
			})
			.for_each(|test_result| match test_result {
				Ok(result) => {
					println!("{}", result.output);
				},
				Err(err) => println!("{}", format!("Error: {}", err).red()),
			});

		Ok(Default::default())
	}
}
