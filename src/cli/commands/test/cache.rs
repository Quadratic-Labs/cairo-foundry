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
use std::{io::BufReader, path::StripPrefixError};

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
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Cache {
	pub path: String,
	pub sha256: String,
}

pub fn create_compiled_contract_path(
	path_to_contract_file: &PathBuf,
	root: &PathBuf,
) -> Result<PathBuf, CacheError> {
	let cache_dir = dirs::cache_dir().ok_or(CacheError::CacheDirSupported)?;
	let root_parent = root.parent().ok_or(CacheError::CacheDirSupported)?;
	let relative_path = path_to_contract_file.strip_prefix(root_parent)?;

	let mut path_to_compiled_contract_path = PathBuf::new();
	path_to_compiled_contract_path.push(&cache_dir);
	path_to_compiled_contract_path.push("compiled-cairo-files");
	path_to_compiled_contract_path.push(&relative_path);
	path_to_compiled_contract_path.set_extension("json");
	Ok(path_to_compiled_contract_path)
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
