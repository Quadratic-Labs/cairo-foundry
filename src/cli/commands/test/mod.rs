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

use self::cache::read_cache;
use self::run::run_tests_for_one_file;

use super::{list::path_is_valid_directory, CommandExecution};
use std::fs;
use thiserror::Error;

use crate::{
	cairo_run::cairo_run,
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

pub mod cache;
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

#[derive(Debug, PartialEq, Eq)]
pub enum TestStatus {
	SUCCESS,
	FAILURE,
}

/// Structure representing the result of one or multiple test.
/// Contains the output of the test, as well as the status.
pub struct TestResult {
	pub output: String,
	pub success: TestStatus,
}

enum CacheStatus {
	Cached,
	Uncached,
}

pub struct CompiledCacheFile {
	path: PathBuf,
	status: CacheStatus,
}

#[derive(Serialize, Deserialize, Debug)]
struct CacheJson {
	contract_path: String,
	sha256: String,
}

fn compute_hash(filepath: &PathBuf) -> Result<String, String> {
	// hash filepath
	let mut hasher = Sha256::new();
	let mut file = File::open(filepath).map_err(|e| format!("Failed to open file: {}", e))?;
	io::copy(&mut file, &mut hasher).map_err(|e| format!("Failed to hash file: {}", e))?;
	let hash = hasher.finalize();
	return Ok(format!("{:x}", hash));
}

impl From<(String, TestStatus)> for TestResult {
	fn from(from: (String, TestStatus)) -> Self {
		Self {
			output: from.0,
			success: from.1,
		}
	}
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

/// Compile a cairo file, returning a truple
/// (path_to_original_code, path_to_compiled_code, entrypoints)
fn compile_and_list_entrypoints_original(
	path_to_code: PathBuf,
) -> Result<(PathBuf, PathBuf, Vec<String>), TestCommandError> {
	let path_to_compiled = compile(&path_to_code)?;
	let entrypoints = list_test_entrypoints(&path_to_compiled)?;
	Ok((path_to_code, path_to_compiled, entrypoints))
}

fn compile_and_list_entrypoints(
	cache: Result<CompiledCacheFile, String>,
) -> Option<(PathBuf, PathBuf, Vec<String>)> {
	match cache {
		Ok(cache) => match cache.status {
			CacheStatus::Cached => {
				println!(
					"Using cached compiled file {}",
					cache.path.display().to_string()
				);
				let compiled_path = cache.path.clone();
				let entrypoints =
					list_test_entrypoints(&cache.path).expect("Failed to list entrypoints");
				return Some((cache.path, compiled_path, entrypoints));
			},
			CacheStatus::Uncached => {
				let compiled_path = compile(&cache.path).expect("Failed to compile");
				let entrypoints =
					list_test_entrypoints(&compiled_path).expect("Failed to list entrypoints");
				return Some((cache.path, compiled_path, entrypoints));
			},
		},
		Err(err) => {
			eprintln!("{}", err);
			return None;
		},
	}
}

fn create_compiled_contract_path(path_to_code: &PathBuf) -> PathBuf {
	let filename = path_to_code.file_stem().expect("File does not have a file stem");

	let cache_dir = dirs::cache_dir().expect("Could not make cache directory");
	let mut path_to_compiled = PathBuf::new();
	path_to_compiled.push(&cache_dir);
	path_to_compiled.push("compiled-cairo-files");
	path_to_compiled.push(filename);
	path_to_compiled.set_extension("json");
	return path_to_compiled;
}

fn dump_json_file(path: &PathBuf, data: &CacheJson) -> Result<(), String> {
	let file = File::create(path).map_err(|op| format!("file does not exists {}", op))?;
	let writer = BufWriter::new(file);
	serde_json::to_writer_pretty(writer, data)
		.map_err(|op| format!("file does not exists {}", op))?;
	return Ok(());
}

fn get_cache(path_to_code: PathBuf) -> Result<CompiledCacheFile, String> {
	// read individual cache file
	// avoid same cache file because we're doing multiprocessing and getting race condition
	let cache_dir = dirs::cache_dir().expect("cache dir not supported");
	let filename = path_to_code.file_stem().unwrap().to_str().unwrap();

	let mut cache_path = PathBuf::new();
	cache_path.push(&cache_dir);
	cache_path.push("cairo-foundry-cache");

	// create dir if not exist to store cache files
	// cache dir will be in os_cache_dir/cairo-foundry-cache
	// os_cache_dir is different for each os
	if !cache_path.exists() {
		std::fs::create_dir(&cache_path).expect("Could not make cache directory");
	}
	// cache file will be in os_cache_dir/cairo-foundry-cache/contract_name.json
	cache_path.push(format!("{}.json", filename));

	let data = read_cache(&cache_path);
	// compute hash from file
	let hash_calculated = compute_hash(&path_to_code).unwrap();
	let contract_path = path_to_code.to_str().unwrap().to_string();

	match data {
		// json file exists
		Ok(cache_data) => {
			let compiled_contract_path = create_compiled_contract_path(&path_to_code);
			let hash_in_cache = cache_data.sha256;
			if *hash_in_cache == hash_calculated {
				return Ok(CompiledCacheFile {
					path: compiled_contract_path,
					status: CacheStatus::Cached,
				});
			} else {
				let data = CacheJson {
					contract_path,
					sha256: hash_calculated,
				};

				dump_json_file(&cache_path, &data)?;
				return Ok(CompiledCacheFile {
					path: path_to_code,
					status: CacheStatus::Uncached,
				});
			}
		},

		// json file does not exists
		Err(_) => {
			let data = CacheJson {
				contract_path,
				sha256: hash_calculated,
			};
			dump_json_file(&cache_path, &data)?;
			return Ok(CompiledCacheFile {
				path: path_to_code,
				status: CacheStatus::Uncached,
			});
		},
	}
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
