use std::collections::{HashMap, HashSet};

use http_client::http::uri::PathAndQuery;
use prost::Message as _;
use prost_reflect::{DescriptorPool, prost_types::FileDescriptorProto};
use tonic::{Code, Status, client::Grpc, metadata::MetadataMap, transport::Channel};
use tonic_prost::ProstCodec;
use tonic_reflection::pb::v1::{
    ServerReflectionRequest, ServerReflectionResponse, server_reflection_request::MessageRequest,
    server_reflection_response::MessageResponse,
};

use super::{GrpcError, GrpcStatus, ServiceDefinition, transport};

// v1alpha has the same messages under an older name. Servers built before v1
// only offer v1alpha, so try it when v1 is not implemented.
const V1: &str = "/grpc.reflection.v1.ServerReflection/ServerReflectionInfo";
const V1_ALPHA: &str = "/grpc.reflection.v1alpha.ServerReflection/ServerReflectionInfo";

/// Ask the server for its services and every file they depend on.
pub(crate) async fn load(
    channel: Channel,
    metadata: MetadataMap,
) -> Result<ServiceDefinition, GrpcError> {
    let files = match files(channel.clone(), &metadata, V1).await {
        Err(status) if status.code() == Code::Unimplemented => {
            files(channel, &metadata, V1_ALPHA).await
        }
        result => result,
    }
    .map_err(|status| match status.code() {
        Code::Unimplemented => GrpcError::Reflection(
            "the server does not support reflection. Import a .proto file instead".into(),
        ),
        _ => match transport::connection_error(&status) {
            Some(error) => GrpcError::Connect(error),
            None => GrpcError::Reflection(GrpcStatus::from(&status).to_string()),
        },
    })?;

    ServiceDefinition::from_files(files)
}

async fn files(
    channel: Channel,
    metadata: &MetadataMap,
    path: &'static str,
) -> Result<Vec<FileDescriptorProto>, Status> {
    let mut reflection = Reflection {
        grpc: Grpc::new(channel),
        metadata,
        path,
    };

    // The field is unused, but some servers, such as Node's, ignore an
    // empty request. grpcurl sends "*" too.
    let services = match reflection
        .ask(MessageRequest::ListServices("*".into()))
        .await?
    {
        MessageResponse::ListServicesResponse(list) => list
            .service
            .into_iter()
            .map(|service| service.name)
            .filter(|name| !name.starts_with("grpc.reflection."))
            .collect::<Vec<_>>(),
        _ => return Err(Status::internal("unexpected reply to the service list")),
    };

    let mut files = HashMap::new();
    let mut missing = Vec::new();
    let mut unavailable = Vec::new();

    for service in services {
        match reflection
            .ask(MessageRequest::FileContainingSymbol(service.clone()))
            .await
        {
            Ok(MessageResponse::FileDescriptorResponse(response)) => {
                add_files(response.file_descriptor_proto, &mut files, &mut missing)?;
            }
            // One unresolvable service should not hide the others.
            Err(status) if status.code() == Code::NotFound => unavailable.push(service),
            Err(status) => return Err(status),
            Ok(_) => return Err(Status::internal("unexpected reply to a service lookup")),
        }
    }

    let mut requested = HashSet::new();

    while let Some(name) = missing.pop() {
        if files.contains_key(&name) || !requested.insert(name.clone()) {
            continue;
        }

        // Some servers omit well-known files; the pool supplies those.
        if DescriptorPool::global().get_file_by_name(&name).is_some() {
            continue;
        }

        match reflection
            .ask(MessageRequest::FileByFilename(name.clone()))
            .await?
        {
            MessageResponse::FileDescriptorResponse(response) => {
                add_files(response.file_descriptor_proto, &mut files, &mut missing)?;
            }
            _ => return Err(Status::internal("unexpected reply to a file lookup")),
        }
    }

    if files.is_empty() && !unavailable.is_empty() {
        return Err(Status::not_found(format!(
            "the server did not describe {}",
            unavailable.join(", ")
        )));
    }

    Ok(files.into_values().collect())
}

struct Reflection<'a> {
    grpc: Grpc<Channel>,
    metadata: &'a MetadataMap,
    path: &'static str,
}

impl Reflection<'_> {
    /// Ask one question on its own stream. Some servers answer only once the
    /// client has finished sending, so every stream carries one request.
    async fn ask(&mut self, request: MessageRequest) -> Result<MessageResponse, Status> {
        self.grpc
            .ready()
            .await
            .map_err(|error| Status::unavailable(transport::error_chain(&error)))?;

        let mut call = tonic::Request::new(futures::stream::iter([ServerReflectionRequest {
            host: String::new(),
            message_request: Some(request),
        }]));
        *call.metadata_mut() = self.metadata.clone();

        let response = self
            .grpc
            .streaming(
                call,
                PathAndQuery::from_static(self.path),
                ProstCodec::<ServerReflectionRequest, ServerReflectionResponse>::default(),
            )
            .await?
            .into_inner()
            .message()
            .await?
            .ok_or_else(|| Status::internal("the server sent no reflection reply"))?;

        match response.message_response {
            Some(MessageResponse::ErrorResponse(error)) => Err(Status::new(
                Code::from_i32(error.error_code),
                error.error_message,
            )),
            Some(response) => Ok(response),
            None => Err(Status::internal("empty reflection reply")),
        }
    }
}

fn add_files(
    encoded: Vec<Vec<u8>>,
    files: &mut HashMap<String, FileDescriptorProto>,
    missing: &mut Vec<String>,
) -> Result<(), Status> {
    for bytes in encoded {
        let file = FileDescriptorProto::decode(bytes.as_slice())
            .map_err(|error| Status::internal(format!("invalid file descriptor: {error}")))?;

        missing.extend(
            file.dependency
                .iter()
                .filter(|name| !files.contains_key(*name))
                .cloned(),
        );
        files.insert(file.name().to_owned(), file);
    }

    Ok(())
}
