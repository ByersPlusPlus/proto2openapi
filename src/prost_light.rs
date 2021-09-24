use std::{path::Path, process::Command};

use prost_types::FileDescriptorSet;
use prost::Message;

/// A trait for getting the FieDescriptorSet from a `prost_build::Config`
pub trait GetProtoFileDescriptor {
    /// Invokes protoctl to get the FileDescriptorSet
    fn get_descriptor(&mut self, protos: &[impl AsRef<Path>], includes: &[impl AsRef<Path>]) -> Result<prost_types::FileDescriptorSet, Box<dyn std::error::Error>>;
}

impl GetProtoFileDescriptor for prost_build::Config {
    /// Invokes protoctl to get the FileDescriptorSet
    fn get_descriptor(&mut self, protos: &[impl AsRef<Path>], includes: &[impl AsRef<Path>]) -> Result<prost_types::FileDescriptorSet, Box<dyn std::error::Error>> {
        let tmp = tempfile::Builder::new().prefix("prost-light-build").tempdir()?;
        let descriptor_path = tmp.path().join("prost-light-descriptor-set");

        let mut cmd = Command::new(prost_build::protoc());
        cmd.arg("--include_imports")
            .arg("--include_source_info")
            .arg("-o")
            .arg(&descriptor_path);
        
        for include in includes {
            cmd.arg("-I").arg(include.as_ref());
        }

        cmd.arg("-I").arg(prost_build::protoc_include());

        for proto in protos {
            cmd.arg(proto.as_ref());
        }

        let output = cmd.output().map_err(|error| {
            std::io::Error::new(error.kind(), format!("failed to invoke protoc (hint: https://docs.rs/prost-build/#sourcing-protoc): {}", error),)
        })?;

        if !output.status.success() {
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, format!("protoc failed: {}", String::from_utf8_lossy(&output.stderr)))));
        }

        let buf = std::fs::read(descriptor_path)?;
        let file_descriptor_set = FileDescriptorSet::decode(&*buf).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("failed to decode FileDescriptorSet: {}", error),)
        })?;
        
        Ok(file_descriptor_set)
    }
}