mod prost_light;
mod openapi_gen;

use std::path::Path;

use clap::load_yaml;
use openapi_gen::OpenAPIGenerator;

/// Main function of the tool
fn main() {
    let yaml = load_yaml!("cli.yml");
    let matches = clap::App::from_yaml(yaml).get_matches();

    let protos = matches.values_of("proto").unwrap();
    let protos: Vec<&Path> = protos.map(|p| Path::new(p)).collect();
    let proto_dirs = protos.iter().map(|p| p.parent().unwrap()).collect::<Vec<_>>();
    let openapi_path = Path::new(matches.value_of("OUTPUT").unwrap());
    let openapi_title = matches.value_of("openapi-title").unwrap();
    let openapi_version = matches.value_of("openapi-version").unwrap();

    let mut config = prost_build::Config::new();
    let mut openapi = OpenAPIGenerator::generate(&mut config, &protos, &proto_dirs);

    openapi.info.title = openapi_title.to_string();
    openapi.info.version = openapi_version.to_string();

    let file = match std::fs::File::create(openapi_path) {
        Ok(file) => file,
        Err(err) => {
            panic!("Failed to create file: {}", err);
        }
    };
    serde_yaml::to_writer(file, &openapi).unwrap();
}
