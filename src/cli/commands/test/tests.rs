use cairo_rs::serde::deserialize_program::deserialize_program_json;

use crate::cli::commands::{test::TestArgs, CommandExecution};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use super::{
	compile_and_list_entrypoints, read_cache, setup_hint_processor, setup_hooks,
	test_single_entrypoint, TestCommandError, TestResult,
};

use crate::cli::commands::test::cache::{
	create_compiled_contract_path, read_json_file, CacheJson, Error,
};

pub fn run_single_test(
	test_name: &str,
	test_path: &PathBuf,
	max_steps: u64,
) -> Result<TestResult, TestCommandError> {
	let (_, path_to_compiled, _) =
		compile_and_list_entrypoints(read_cache(test_path.to_owned())).unwrap();

	let file = File::open(path_to_compiled).unwrap();
	let reader = BufReader::new(file);
	let program_json = deserialize_program_json(reader)?;

	test_single_entrypoint(
		program_json,
		test_name,
		&mut setup_hint_processor(),
		Some(setup_hooks()),
		max_steps,
	)
}

#[test]
fn test_cairo_contracts() {
	let root = PathBuf::from("test_cairo_contracts");
	let mut absolute_path = std::env::current_dir().unwrap();
	absolute_path.push(root);

	println!("Testing {}", absolute_path.as_path().display().to_string());
	TestArgs {
		root: absolute_path,
		max_steps: 1000000,
	}
	.exec()
	.unwrap();
}

#[test]
fn test_create_compiled_contract_path_positive_0() {
	let current_dir = std::env::current_dir().unwrap();
	let root = PathBuf::from(current_dir.join("test_cairo_contracts"));

	let path_to_contract_file = PathBuf::from(root.join("test_valid_program.cairo"));
	let path_to_compiled_contract_path =
		create_compiled_contract_path(&path_to_contract_file, &root).unwrap();
	let cache_dir = dirs::cache_dir().ok_or(Error::CacheDirSupported).unwrap();
	assert_eq!(
		path_to_compiled_contract_path,
		cache_dir.join("compiled-cairo-files/test_cairo_contracts/test_valid_program.json")
	);
}

#[test]
fn test_create_compiled_contract_path_positive_1() {
	let current_dir = std::env::current_dir().unwrap();
	let root = PathBuf::from(current_dir.join("test_cairo_contracts"));
	let path_to_contract_file = PathBuf::from(root.join("test_valid_program.cairo"));
	let path_to_compiled_contract_path =
		create_compiled_contract_path(&path_to_contract_file, &root).unwrap();
	let cache_dir = dirs::cache_dir().ok_or(Error::CacheDirSupported).unwrap();
	assert_eq!(
		path_to_compiled_contract_path,
		cache_dir.join("compiled-cairo-files/test_cairo_contracts/test_valid_program.json")
	);
}

#[test]
fn test_read_json_positive_0() {
	let current_dir = std::env::current_dir().unwrap();
	let root = PathBuf::from(current_dir.join("test_compiled_contracts"));
	let path_to_compiled_contract_path = PathBuf::from(root.join("test_valid_program.json"));
	let json = read_json_file(&path_to_compiled_contract_path).unwrap();

	let expected_json = CacheJson {
		contract_path: "test_compiled_contracts/test_valid_program.cairo".to_string(),
		sha256: "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
	};

	assert_eq!(json, expected_json);
}
