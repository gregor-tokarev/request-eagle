use std::path::{Path, PathBuf};

use prost_reflect::prost_types::FileDescriptorProto;
use prost_reflect::{
    DescriptorPool, DynamicMessage, MessageDescriptor, MethodDescriptor, SerializeOptions,
};

use super::GrpcError;

/// Services and message types available to a gRPC request, from server
/// reflection or a compiled `.proto` file.
#[derive(Clone, Debug)]
pub struct ServiceDefinition {
    pool: DescriptorPool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MethodKind {
    Unary,
    ClientStreaming,
    ServerStreaming,
    BidiStreaming,
}

impl MethodKind {
    /// Whether the client sends its messages one by one after invoking.
    pub fn streams_requests(self) -> bool {
        matches!(self, Self::ClientStreaming | Self::BidiStreaming)
    }

    pub fn streams_responses(self) -> bool {
        matches!(self, Self::ServerStreaming | Self::BidiStreaming)
    }

    fn of(method: &MethodDescriptor) -> Self {
        match (method.is_client_streaming(), method.is_server_streaming()) {
            (false, false) => Self::Unary,
            (true, false) => Self::ClientStreaming,
            (false, true) => Self::ServerStreaming,
            (true, true) => Self::BidiStreaming,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrpcService {
    /// The fully qualified name, such as `package.Service`.
    pub name: String,
    pub methods: Vec<GrpcMethod>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrpcMethod {
    pub name: String,
    /// The value stored in `GrpcRequest::method`: `package.Service/Method`.
    pub path: String,
    pub kind: MethodKind,
}

impl ServiceDefinition {
    /// Compile a `.proto` file and its imports. Imports resolve from each
    /// import path in order, then from the file's directory; well-known
    /// `google/protobuf` files are built in.
    pub fn from_proto_file(path: &Path, import_paths: &[PathBuf]) -> Result<Self, GrpcError> {
        let path = std::path::absolute(path)
            .map_err(|error| GrpcError::ProtoFile(format!("{}: {error}", path.display())))?;

        if !path.is_file() {
            return Err(GrpcError::ProtoFile(format!(
                "{} does not exist",
                path.display()
            )));
        }

        let mut includes = import_paths
            .iter()
            .filter_map(|path| std::path::absolute(path).ok())
            .filter(|include| path.starts_with(include))
            .collect::<Vec<_>>();
        // Import paths that do not contain the file can still provide its
        // imports, but only an include that contains it can name the file.
        includes.extend(
            import_paths
                .iter()
                .filter_map(|path| std::path::absolute(path).ok())
                .filter(|include| !path.starts_with(include)),
        );
        includes.extend(path.parent().map(Path::to_path_buf));

        let pool = protox::Compiler::new(&includes)
            .and_then(|mut compiler| {
                compiler.include_imports(true).open_file(&path)?;
                Ok(compiler.descriptor_pool())
            })
            // The debug format starts with the file, line and column.
            .map_err(|error| GrpcError::ProtoFile(format!("{error:?}")))?;

        Ok(Self { pool })
    }

    /// Build a definition from file descriptors in any order.
    pub(crate) fn from_files(files: Vec<FileDescriptorProto>) -> Result<Self, GrpcError> {
        // Some servers leave out the well-known files they import. The global
        // pool has those; files the server did send are skipped as duplicates.
        let mut pool = DescriptorPool::global();
        pool.add_file_descriptor_protos(files)
            .map_err(|error| GrpcError::Reflection(error.to_string()))?;

        Ok(Self { pool })
    }

    /// Services in name order, excluding the reflection service itself.
    pub fn services(&self) -> Vec<GrpcService> {
        let mut services = self
            .pool
            .services()
            .filter(|service| !service.full_name().starts_with("grpc.reflection."))
            .map(|service| GrpcService {
                name: service.full_name().to_owned(),
                methods: service
                    .methods()
                    .map(|method| GrpcMethod {
                        name: method.name().to_owned(),
                        path: format!("{}/{}", service.full_name(), method.name()),
                        kind: MethodKind::of(&method),
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        services.sort_by(|a, b| a.name.cmp(&b.name));

        services
    }

    pub fn method(&self, path: &str) -> Option<GrpcMethod> {
        let descriptor = self.descriptor(path)?;

        Some(GrpcMethod {
            name: descriptor.name().to_owned(),
            path: path.to_owned(),
            kind: MethodKind::of(&descriptor),
        })
    }

    pub(crate) fn descriptor(&self, path: &str) -> Option<MethodDescriptor> {
        let (service, method) = path.trim_start_matches('/').split_once('/')?;

        self.pool
            .get_service_by_name(service)?
            .methods()
            .find(|candidate| candidate.name() == method)
    }

    /// A JSON message with every field of the method's input filled in.
    pub fn example_message(&self, path: &str) -> Option<String> {
        let input = self.descriptor(path)?.input();

        Some(super::example::message(&input))
    }
}

/// Parse a JSON message; an empty text is an empty message.
pub(crate) fn parse_message(
    descriptor: &MessageDescriptor,
    text: &str,
) -> Result<DynamicMessage, GrpcError> {
    let text = if text.trim().is_empty() { "{}" } else { text };
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let message = DynamicMessage::deserialize(descriptor.clone(), &mut deserializer)
        .and_then(|message| deserializer.end().map(|()| message))
        .map_err(|error| GrpcError::InvalidMessage(error.to_string()))?;

    Ok(message)
}

/// Format a message as indented JSON. Including fields with default values
/// shows every field of the message.
pub(crate) fn format_message(message: &DynamicMessage, include_defaults: bool) -> String {
    let mut output = Vec::new();
    let mut serializer = serde_json::Serializer::pretty(&mut output);
    let options = SerializeOptions::new().skip_default_fields(!include_defaults);

    match message.serialize_with_options(&mut serializer, &options) {
        Ok(()) => String::from_utf8(output).unwrap_or_default(),
        Err(error) => format!("\"could not format the message: {error}\""),
    }
}
