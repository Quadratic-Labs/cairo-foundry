#[cfg(test)]
mod tests;
use sha2::{Digest, Sha256, Sha512};

use cairo_rs::serde::deserialize_program::ProgramJson;
use dirs;
use serde_json::Value;
use std::{
	fmt::Debug,
	fs::{read_to_string, File},
	io::{self, Write},
	path::{Path, PathBuf},
	process::Command,
};
use thiserror::Error;

use serde::{Deserialize, Serialize};
use std::{io::BufReader, io::BufWriter, path::StripPrefixError};

use serde_json;

#[derive(Error, Debug)]
pub enum CacheError {
	#[error("StripPrefixError: {0}")]
	StripPrefixError(#[from] StripPrefixError),
	#[error("failed to execute a process: {0}")]
	RunProcess(io::Error),
	#[error("binary '{0}' failed to compile '{1}'")]
	Compilation(String, String),
	#[error("file '{0}' has no stem")]
	StemlessFile(String),
	#[error("cache directory does not exist on this platform")]
	CacheDirSupported,
	#[error("failed to create file '{0}': {1}")]
	FileCreation(String, io::Error),
	#[error("failed to create directory '{0}': {1}")]
	DirCreation(String, io::Error),
	#[error("failed to write to file '{0}': {1}")]
	WriteToFile(String, io::Error),
	#[error("failed to read file '{0}': {1}")]
	FileNotFound(PathBuf, io::Error),
	#[error("failed to read file '{0}': {1}")]
	DeserializeError(String, serde_json::Error),
	#[error(transparent)]
	FileNotFoundTransparent(#[from] io::Error),
	#[error(transparent)]
	DeserializeErrorTransparent(#[from] serde_json::Error),
	#[error("cache directory does not exist on this platform")]
	CacheDirNotSupportedError,
	#[error("filename does not exist")]
	InvalidContractExtension(PathBuf),
	// #[error(transparent)]
	// StripPrefixError(#[from] std::path::StripPrefixError),
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Cache {
	pub path: String,
	pub sha256: String,
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

// #[derive(Error, Debug)]
// pub enum CacheError {}

const CAIRO_FOUNDRY_CACHE_DIR: &str = "cairo-foundry-cache";
const CAIRO_FOUNDRY_COMPILED_CONTRACT_DIR: &str = "compiled-cairo-files";

fn compute_hash(filepath: &PathBuf) -> Result<String, String> {
	// hash filepath
	let mut hasher = Sha256::new();
	let mut file = File::open(filepath).map_err(|e| format!("Failed to open file: {}", e))?;
	io::copy(&mut file, &mut hasher).map_err(|e| format!("Failed to hash file: {}", e))?;
	let hash = hasher.finalize();
	return Ok(format!("{:x}", hash));
}

// pub fn create_compiled_contract_path(
// 	path_to_contract_file: &PathBuf,
// 	root: &PathBuf,
// ) -> Result<PathBuf, CacheError> {
// 	let cache_dir = dirs::cache_dir().ok_or(CacheError::CacheDirSupported)?;
// 	let root_parent = root.parent().ok_or(CacheError::CacheDirSupported)?;
// 	let relative_path = path_to_contract_file.strip_prefix(root_parent)?;

// 	let mut path_to_compiled_contract_path = PathBuf::new();
// 	path_to_compiled_contract_path.push(&cache_dir);
// 	path_to_compiled_contract_path.push("compiled-cairo-files");
// 	path_to_compiled_contract_path.push(&relative_path);
// 	path_to_compiled_contract_path.set_extension("json");
// 	Ok(path_to_compiled_contract_path)
// }

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

pub fn read_cache(path: &PathBuf) -> Result<Cache, CacheError> {
	let file = read_to_string(path).map_err(|op| CacheError::FileNotFound(path.to_owned(), op))?;
	let data = serde_json::from_str::<Cache>(file.as_str())
		.map_err(|op| CacheError::DeserializeError(file, op))?;
	Ok(data)
}

pub fn write_cache(path: &PathBuf, cache: Cache) -> Result<(), CacheError> {
	Ok(())
}

fn read_cache_file(path: &PathBuf) -> Result<Cache, CacheError> {
	let file = read_to_string(path)?;
	let data = serde_json::from_str::<Cache>(file.as_str())?;
	Ok(data)
}

fn is_valid_cairo_contract(contract_path: &PathBuf) -> Result<(), CacheError> {
	let extension = contract_path
		.extension()
		.ok_or_else(|| CacheError::InvalidContractExtension(contract_path.to_owned()))?;
	if extension != "cairo" {
		return Err(CacheError::InvalidContractExtension(
			contract_path.to_owned(),
		));
	}
	Ok(())
}

fn get_cache_path(contract_path: &PathBuf, root_dir: &PathBuf) -> Result<PathBuf, CacheError> {
	// check if contract_path have .cairo extension
	is_valid_cairo_contract(contract_path)?;
	let cache_dir = dirs::cache_dir().ok_or(CacheError::CacheDirNotSupportedError)?;
	// get relative dir path from root_dir
	let contract_relative_path = contract_path.strip_prefix(root_dir)?;

	let mut cache_path = cache_dir.join(CAIRO_FOUNDRY_CACHE_DIR).join(contract_relative_path);
	cache_path.set_extension("json");
	Ok(cache_path)
}

fn get_compiled_contract_path(
	contract_path: &PathBuf,
	root_dir: &PathBuf,
) -> Result<PathBuf, CacheError> {
	// check if contract_path have .cairo extension
	is_valid_cairo_contract(contract_path)?;
	let cache_dir = dirs::cache_dir().ok_or(CacheError::CacheDirNotSupportedError)?;
	let contract_relative_path = contract_path.strip_prefix(root_dir)?;
	let mut compiled_contract_path =
		cache_dir.join(CAIRO_FOUNDRY_COMPILED_CONTRACT_DIR).join(contract_relative_path);
	compiled_contract_path.set_extension("json");
	Ok(compiled_contract_path)
}

fn dump_json_file(path: &PathBuf, data: &CacheJson) -> Result<(), String> {
	let file = File::create(path).map_err(|op| format!("file does not exists {}", op))?;
	let writer = BufWriter::new(file);
	serde_json::to_writer_pretty(writer, data)
		.map_err(|op| format!("file does not exists {}", op))?;
	return Ok(());
}

pub fn get_cache(path_to_code: PathBuf) -> Result<CompiledCacheFile, String> {
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

pub fn compile_and_list_entrypoints(
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
