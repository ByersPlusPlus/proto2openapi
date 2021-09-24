use std::{collections::HashMap, convert::TryFrom, path::Path};

use indexmap::IndexMap;
use itertools::{Either, Itertools};
use lazy_static::lazy_static;
use multimap::MultiMap;
use openapiv3::{ArrayType, Components, IntegerType, MediaType, NumberType, ObjectType, OpenAPI, Operation, Parameter, ParameterData, ParameterSchemaOrContent, PathStyle, ReferenceOr, RequestBody, Response, Responses, Schema, SchemaData, SchemaKind, StatusCode, StringType, Type};
use prost_build::{Comments, Config, Method, Service};
use prost_types::{DescriptorProto, EnumValueDescriptorProto, FieldDescriptorProto, OneofDescriptorProto, ServiceDescriptorProto, SourceCodeInfo, field_descriptor_proto::{self, Label}, source_code_info::Location};
use regex::Regex;

use super::prost_light::GetProtoFileDescriptor;

/// Allows to convert a location to a `Comments` object.
pub trait Commentable {
    fn from_location(location: &Location) -> Comments;
}

impl Commentable for Comments {
    fn from_location(location: &Location) -> Comments {
        fn get_lines<S>(comments: S) -> Vec<String>
        where
            S: AsRef<str>,
        {
            comments.as_ref().lines().map(str::to_owned).collect()
        }
        let leading_detached = location
            .leading_detached_comments
            .iter()
            .map(get_lines)
            .collect();
        let leading = location
            .leading_comments
            .as_ref()
            .map_or(Vec::new(), get_lines);
        let trailing = location
            .trailing_comments
            .as_ref()
            .map_or(Vec::new(), get_lines);
        Comments {
            leading_detached,
            leading,
            trailing,
        }
    }
}

// The heart of the path generation.
lazy_static! {
    static ref METHOD_RE: Regex = Regex::new(r"^\s*(GET|PUT|POST|DELETE)").unwrap();
    static ref PATH_RE: Regex = Regex::new(r"(?:/(?:(?:\w+)|(?:\{\w+:\w+\})))+").unwrap();
    static ref PARAM_RE: Regex = Regex::new(r"\{(?P<param>\w+):(?P<param_type>\w+)\}").unwrap();
    static ref BODY_RE: Regex = Regex::new(r"(\+|-) BODY").unwrap();
    static ref TAG_RE: Regex = Regex::new(r"\[([a-zA-Z0-9, ]+)\]").unwrap();
}

/// Contains path information for a given proto method.
pub struct OpenAPIPathInfo {
    /// The query path.
    pub path: String,
    /// The query method.
    pub method: String,
    /// The query parameters. Empty if no parameters are present.
    pub parameters: HashMap<String, String>,
    /// `true` if the method should include a body. Defaults to `true`.
    pub include_body: bool,
    /// The path tags.
    pub tags: Vec<String>,
}

/// Converts a query path from a proto comment to a valid OpenAPI path.
pub fn path_to_openapi_path(path: &str) -> String {
    PARAM_RE.replace_all(path, "{$1}").to_string()
}

impl TryFrom<&String> for OpenAPIPathInfo {
    type Error = ();

    /// Converts a proto comment to a path definition.
    fn try_from(value: &String) -> Result<Self, Self::Error> {
        let method = METHOD_RE.captures(value).map(|c| c.get(0).unwrap().as_str().to_owned());
        if method.is_none() {
            return Err(());
        }
        let method = method.unwrap().trim().to_string();
        let path = PATH_RE.captures(value).map(|c| c.get(0).unwrap().as_str().to_owned());
        if path.is_none() {
            return Err(());
        }
        let path = path.unwrap();
        let parameters = PARAM_RE.captures_iter(value).map(|c| {
            let param = c.name("param").unwrap().as_str().to_owned();
            let param_type = c.name("param_type").unwrap().as_str().to_owned();
            (param, param_type)
        }).collect();

        let mut include_body = BODY_RE.is_match(value);
        if include_body {
            let prefix = BODY_RE.captures(value).unwrap().get(1).unwrap().as_str();
            if prefix == "+" {
                include_body = true;
            } else {
                include_body = false;
            }
        } else {
            // if the regex doesn't match, default to true
            include_body = true;
        }

        let mut tags = Vec::new();
        if TAG_RE.is_match(value) {
            let tag_str = TAG_RE.captures(value).unwrap().get(1).unwrap();
            tags = tag_str.as_str().split(',').map(str::trim).map(str::to_owned).collect();
        }

        Ok(OpenAPIPathInfo {
            path,
            method,
            parameters,
            include_body,
            tags,
        })
    }
}

/// Contains information about the generation of the proto files.
pub struct OpenAPIGenerator<'a> {
    pub config: &'a mut Config,
    source_info: SourceCodeInfo,
    path: Vec<i32>,
}

impl<'a> OpenAPIGenerator<'a> {
    /// Returns the current location in the proto file.
    /// This is not accurate!
    pub fn location(&self) -> &Location {
        let idx = self
            .source_info
            .location
            .binary_search_by_key(&&self.path[..], |location| &location.path[..])
            .unwrap();

        &self.source_info.location[idx]
    }

    /// Generates an OpenAPI object, which can be directly serialized to YAML.
    pub fn generate(
        config: &mut Config,
        protos: &[impl AsRef<Path>],
        includes: &[impl AsRef<Path>],
    ) -> OpenAPI {
        let files = config.get_descriptor(protos, includes);
        let files = files.unwrap().file;
        let mut openapi = OpenAPI::default();

        let mut schema_map: IndexMap<String, ReferenceOr<Schema>> = IndexMap::new();
        for file in files {
            let mut source_info = file.source_code_info.clone().expect("");
            source_info.location.retain(|location| {
                let len = location.path.len();
                len > 0 && len % 2 == 0
            });
            source_info
                .location
                .sort_by_key(|location| location.path.clone());

            let mut gen = OpenAPIGenerator {
                config,
                source_info,
                path: Vec::new(),
            };

            gen.path.push(4);
            for (idx, message) in file.message_type.into_iter().enumerate() {
                // generate messages as schemas
                gen.path.push(idx as i32);
                println!("generating message {}", message.name());
                let schema = gen.generate_schema_recursive(message, 0);
                schema_map.extend(schema.into_iter().map(|(k, v)| (k, ReferenceOr::Item(v))));
                gen.path.pop();
            }
            gen.path.pop();

            gen.path.push(5);
            for (idx, enum_type) in file.enum_type.iter().enumerate() {
                gen.path.push(idx as i32);
                println!("generating enum {}", enum_type.name());
                let schema = gen.generate_enum_schema(&enum_type.value);
                schema_map.insert(enum_type.name().to_string(), ReferenceOr::Item(schema));
                gen.path.pop();
            }
            gen.path.pop();

            gen.path.push(6);
            for (idx, service) in file.service.into_iter().enumerate() {
                // generate services as paths
                gen.path.push(idx as i32);
                println!("generating service {}", service.name());
                let svc = gen.generate_service(service);

                let method_infos = svc.methods.into_iter()
                    .map(|m| {
                        let input_type = m.input_proto_type;
                        let output_type = m.output_proto_type;
                        let mut possible_paths = Vec::new();
                        for comment in &m.comments.leading {
                            let path_def = OpenAPIPathInfo::try_from(comment);
                            if let Ok(path_def) = path_def { possible_paths.push(path_def) }
                        }
                        (input_type, output_type, possible_paths)
                    }).collect_vec();
                // collect all possible unique paths
                let mut paths = HashMap::new();
                for (input_type, output_type, possible_paths) in method_infos {
                    for path in possible_paths {
                        if !paths.contains_key(&path.path) {
                            let mut path_info = Vec::new();
                            let str_path = path.path.clone();
                            path_info.push((input_type.clone(), output_type.clone(), path));
                            paths.insert(str_path, path_info);
                        } else {
                            let path_info = paths.get_mut(&path.path).unwrap();
                            path_info.push((input_type.clone(), output_type.clone(), path));
                        }
                    }
                }

                for (path, path_info) in paths {
                    println!("generating path {}", path);
                    let path_item = gen.generate_path(&path_info);
                    openapi.paths.insert(path_to_openapi_path(&path), ReferenceOr::Item(path_item));
                }
                gen.path.pop();
            }
            gen.path.pop();
        }
        openapi.components = Some(Components {
            security_schemes: IndexMap::new(),
            responses: IndexMap::new(),
            parameters: IndexMap::new(),
            request_bodies: IndexMap::new(),
            headers: IndexMap::new(),
            schemas: schema_map,
            examples: IndexMap::new(),
            links: IndexMap::new(),
            callbacks: IndexMap::new(),
            extensions: IndexMap::new(),
        });
        openapi.openapi = "3.0.0".to_string();

        openapi
    }

    /// Generate an OpenAPI path item from a set of path definitions.
    pub fn generate_path(&self, path_info: &[(String, String, OpenAPIPathInfo)]) -> openapiv3::PathItem {
        let mut path_item = openapiv3::PathItem::default();

        // fill in parameters, if present
        // since the path definitions are grouped before being passed to this function,
        // we can assume that the parameters are in the same order as the path and same for every path definition
        let (_, _, first) = path_info.first().unwrap();
        if !first.parameters.is_empty() {
            for (param, param_type) in &first.parameters {
                path_item.parameters.push(ReferenceOr::Item(Parameter::Path {
                    style: PathStyle::Simple,
                    parameter_data: ParameterData {
                        name: param.clone(),
                        description: None,
                        required: true,
                        deprecated: None,
                        format: ParameterSchemaOrContent::Schema(ReferenceOr::Item(Schema {
                            schema_data: SchemaData::default(),
                            schema_kind: SchemaKind::Type(match param_type.as_str() {
                                "string" => Type::String(StringType::default()),
                                "int" => Type::Integer(IntegerType::default()),
                                _ => Type::String(StringType::default()),
                            }),
                        })),
                        example: None,
                        examples: IndexMap::new(),
                        explode: None,
                        extensions: IndexMap::new(),
                    }
                }));
            }
        }

        for (input_type, output_type, path_def) in path_info {
            let mut body_map = IndexMap::new();
            body_map.insert(
                "application/json".to_string(),
                MediaType {
                    schema: Some(ReferenceOr::ref_(format!("#/components/schemas/{}", input_type.split('.').last().unwrap()).as_str())),
                    ..MediaType::default()
                }
            );

            let mut response_map = IndexMap::new();
            response_map.insert(
                "application/json".to_string(),
                MediaType {
                    schema: Some(ReferenceOr::ref_(format!("#/components/schemas/{}", output_type.split('.').last().unwrap()).as_str())),
                    ..MediaType::default()
                }
            );
            let mut responses = IndexMap::new();
            responses.insert(
                StatusCode::Code(200_u16),
                ReferenceOr::Item(Response {
                    content: response_map,
                    description: format!("A response containing {}", output_type.split('.').last().unwrap()),
                    ..Response::default()
                })
            );

            let operation = openapiv3::Operation {
                request_body: if path_def.method != *"GET" && path_def.include_body {
                    Some(ReferenceOr::Item(RequestBody {
                        content: body_map,
                        ..RequestBody::default()
                    }))
                } else {
                    None
                },
                responses: Responses {
                    default: None,
                    responses,
                },
                tags: path_def.tags.clone(),
                ..Operation::default()
            };

            match path_def.method.as_str() {
                "GET" => {
                    path_item.get = Some(operation);
                },
                "POST" => {
                    path_item.post = Some(operation);
                },
                "PUT" => {
                    path_item.put = Some(operation);
                },
                "DELETE" => {
                    path_item.delete = Some(operation);
                },
                _ => {}
            }
        }

        path_item
    }

    /// Recursively generates an OpenAPI schema from a proto message and its nested messages and enums.
    ///
    /// # Important
    /// This function will flatten all nested messages and enums into a single map.
    /// This is because the OpenAPI spec does not support nested messages and enums.
    pub fn generate_schema_recursive(&self, tl_message: DescriptorProto, mut depth: i32) -> HashMap<String, Schema> {
        depth += 1;
        let mut schema_map = HashMap::new();
        if depth >= 10 {
            // Safety: we are not going to exceed the maximum depth of 10
            return schema_map;
        }
        let message_name = tl_message.name().to_string();
        let oneof_decl = tl_message.oneof_decl;

        for nested_message in tl_message.nested_type {
            let schema = self.generate_schema_recursive(nested_message, depth);
            schema_map.extend(schema);
        }

        type Fields = Vec<FieldDescriptorProto>;
        type OneofFields = MultiMap<i32, FieldDescriptorProto>;
        let (fields, oneof_fields): (Fields, OneofFields) = tl_message
            .field
            .into_iter()
            .enumerate()
            .partition_map(|(_, field)| {
                if field.proto3_optional.unwrap_or(false) {
                    Either::Left(field)
                } else if let Some(oneof_index) = field.oneof_index {
                    Either::Right((oneof_index, field))
                } else {
                    Either::Left(field)
                }
            });
        let tl_schema = self.generate_fields_schema(&fields, &oneof_fields, &oneof_decl);
        schema_map.insert(message_name, tl_schema);

        for enum_descriptor in &tl_message.enum_type {
            let enum_schema = self.generate_enum_schema(&enum_descriptor.value);
            schema_map.insert(enum_descriptor.name().to_string(), enum_schema);
        }

        schema_map
    }

    /// Generates an OpenAPI schema containing an enum, along with a description which contains the possible values.
    pub fn generate_enum_schema(&self, enum_values: &[EnumValueDescriptorProto]) -> Schema {
        let schema_data = SchemaData {
            description: Some(enum_values.iter().map(|e| {
                format!("{} = {}", e.name(), e.number())
            }).join("\n\n")),
            ..SchemaData::default()
        };

        let integer_type = IntegerType {
            enumeration: enum_values.iter().map(|evd| evd.number.unwrap() as i64).collect(),
            ..IntegerType::default()
        };

        let schema_kind = SchemaKind::Type(Type::Integer(integer_type));

        Schema {
            schema_data,
            schema_kind,
        }
    }

    /// Generates an OpenAPI schema containing a message.
    pub fn generate_fields_schema(
        &self,
        fields: &[FieldDescriptorProto],
        oneof_fields: &MultiMap<i32, FieldDescriptorProto>,
        oneof_decl: &[OneofDescriptorProto],
    ) -> Schema {
        let schema_data = SchemaData::default();
        let mut object_type = ObjectType::default();

        for field in fields {
            let field_name = field.name();

            if field.label() == Label::Repeated {
                // type is array
                if field.type_name.is_some() {
                    // type is a foreign type
                    // it could be a reference to an existing schema type or a proto type
                    let field_type_name = field.type_name.as_ref().unwrap();
                    let field_type_name = field_type_name.split('.').last().unwrap().to_string();
                    object_type.properties.insert(
                        field_name.to_string(),
                        ReferenceOr::boxed_item(Schema {
                            schema_kind: SchemaKind::Type(Type::Array(ArrayType {
                                min_items: None,
                                max_items: None,
                                unique_items: false,
                                items: ReferenceOr::ref_(format!("#/components/schemas/{}", field_type_name.as_str()).as_str()),
                            })),
                            schema_data: SchemaData::default(),
                        }),
                    );
                } else {
                    let inner_type = match field.r#type() {
                        field_descriptor_proto::Type::Bool => Type::Boolean {},
                        field_descriptor_proto::Type::String => Type::String(StringType::default()),
                        field_descriptor_proto::Type::Double => Type::Number(NumberType::default()),
                        field_descriptor_proto::Type::Float => Type::Number(NumberType::default()),
                        field_descriptor_proto::Type::Int32 => {
                            Type::Integer(IntegerType::default())
                        }
                        field_descriptor_proto::Type::Int64 => {
                            Type::Integer(IntegerType::default())
                        }
                        field_descriptor_proto::Type::Uint32 => {
                            Type::Integer(IntegerType::default())
                        }
                        field_descriptor_proto::Type::Uint64 => {
                            Type::Integer(IntegerType::default())
                        }
                        _ => Type::String(StringType::default()),
                    };
                    let field_schema: Schema = Schema { schema_data: SchemaData::default(), schema_kind: SchemaKind::Type(inner_type) };
                    object_type.properties.insert(
                        field_name.to_string(),
                        ReferenceOr::boxed_item(Schema {
                            schema_data: SchemaData::default(),
                            schema_kind: SchemaKind::Type(Type::Array(ArrayType {
                                min_items: None,
                                max_items: None,
                                unique_items: false,
                                items: ReferenceOr::boxed_item(field_schema),
                            })),
                        }),
                    );
                }
            } else {
                // type is object
                if field.type_name.is_some() {
                    // type is a foreign type
                    // it could be a reference to an existing schema type or a proto type
                    let field_type_name = field.type_name.as_ref().unwrap();
                    let field_type_name = field_type_name.split('.').last().unwrap().to_string();
                    object_type.properties.insert(
                        field_name.to_string(),
                        ReferenceOr::ref_(format!("#/components/schemas/{}", field_type_name.as_str()).as_str()),
                    );
                } else {
                    let inner_type = match field.r#type() {
                        field_descriptor_proto::Type::Bool => Type::Boolean {},
                        field_descriptor_proto::Type::String => Type::String(StringType::default()),
                        field_descriptor_proto::Type::Double => Type::Number(NumberType::default()),
                        field_descriptor_proto::Type::Float => Type::Number(NumberType::default()),
                        field_descriptor_proto::Type::Int32 => {
                            Type::Integer(IntegerType::default())
                        }
                        field_descriptor_proto::Type::Int64 => {
                            Type::Integer(IntegerType::default())
                        }
                        field_descriptor_proto::Type::Uint32 => {
                            Type::Integer(IntegerType::default())
                        }
                        field_descriptor_proto::Type::Uint64 => {
                            Type::Integer(IntegerType::default())
                        }
                        _ => Type::String(StringType::default()),
                    };
                    let field_schema: Schema = Schema { schema_data: SchemaData::default(), schema_kind: SchemaKind::Type(inner_type) };
                    object_type.properties.insert(
                        field_name.to_string(),
                        ReferenceOr::boxed_item(field_schema),
                    );
                }
            }
        }

        for (idx, oneof) in oneof_decl.iter().enumerate() {
            let idx = idx as i32;

            let oneofs = match oneof_fields.get_vec(&idx) {
                Some(fields) => fields,
                None => continue,
            };

            let field_name = oneof.name();
            let field_schema: Schema = Schema { schema_data: SchemaData::default(), schema_kind: SchemaKind::OneOf {
                one_of: oneofs.iter().map(|o| {
                    let mut ind_map: IndexMap<String, ReferenceOr<Box<Schema>>> = IndexMap::new();
                    ind_map.insert(o.name().to_string(), ReferenceOr::boxed_item(Schema { 
                        schema_data: SchemaData::default(), 
                        schema_kind: SchemaKind::Type(match o.r#type() {
                            field_descriptor_proto::Type::Bool => Type::Boolean {},
                            field_descriptor_proto::Type::String => Type::String(StringType::default()),
                            field_descriptor_proto::Type::Double => Type::Number(NumberType::default()),
                            field_descriptor_proto::Type::Float => Type::Number(NumberType::default()),
                            field_descriptor_proto::Type::Int32 => {
                                Type::Integer(IntegerType::default())
                            }
                            field_descriptor_proto::Type::Int64 => {
                                Type::Integer(IntegerType::default())
                            }
                            field_descriptor_proto::Type::Uint32 => {
                                Type::Integer(IntegerType::default())
                            }
                            field_descriptor_proto::Type::Uint64 => {
                                Type::Integer(IntegerType::default())
                            }
                            _ => Type::String(StringType::default()),
                        })
                    }));

                    ReferenceOr::Item(Schema {
                        schema_data: SchemaData::default(),
                        schema_kind: SchemaKind::Type(Type::Object(ObjectType {
                            properties: ind_map,
                            ..ObjectType::default()
                        })),
                    })
                }).collect(),
            } };

            object_type.properties.insert(
                field_name.to_string(),
                ReferenceOr::boxed_item(field_schema),
            );
        }

        let schema_kind = SchemaKind::Type(Type::Object(object_type));

        Schema {
            schema_data,
            schema_kind,
        }
    }

    /// Generate a service from a service descriptor. Contains comments to the service and its methods.
    pub fn generate_service(&mut self, service: ServiceDescriptorProto) -> Service {
        let name = service.name().to_owned();
        let comments = Comments::from_location(self.location());

        self.path.push(2);
        let methods = service
            .method
            .into_iter()
            .enumerate()
            .map(|(idx, mut method)| {
                self.path.push(idx as i32);
                let comments = Comments::from_location(self.location());
                self.path.pop();

                let name = method.name.take().unwrap();
                let input_proto_type = method.input_type.take().unwrap();
                let output_proto_type = method.output_type.take().unwrap();
                let input_type = "".to_string();
                let output_type = "".to_string();
                let client_streaming = method.client_streaming();
                let server_streaming = method.server_streaming();

                Method {
                    name: name.clone(),
                    proto_name: name,
                    comments,
                    input_type,
                    output_type,
                    input_proto_type,
                    output_proto_type,
                    options: method.options.unwrap_or_default(),
                    client_streaming,
                    server_streaming,
                }
            })
            .collect();
        self.path.pop();

        Service {
            name: name.clone(),
            proto_name: name,
            package: "".to_string(),
            comments,
            methods,
            options: service.options.unwrap_or_default(),
        }
    }
}
