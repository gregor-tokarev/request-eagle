use collection::ImportedItem;
use request::{HttpRequest, Method, Request};
use serde_json::json;

use crate::{ImportError, parse};

fn http(item: &ImportedItem) -> (&str, &HttpRequest) {
    match item {
        ImportedItem::Request {
            name,
            request: Request::Http(request),
        } => (name, request),
        ImportedItem::Folder { name, .. } => panic!("{name} is a folder"),
    }
}

fn folder(item: &ImportedItem) -> (&str, &[ImportedItem]) {
    match item {
        ImportedItem::Folder { name, items } => (name, items),
        ImportedItem::Request { name, .. } => panic!("{name} is a request"),
    }
}

fn json_body(request: &HttpRequest) -> serde_json::Value {
    serde_json::from_slice(request.body.as_ref().unwrap()).unwrap()
}

const PET_STORE: &str = r##"
openapi: 3.0.3
info:
  title: Pet Store
servers:
  - url: https://{region}.pets.test/v1/
    variables:
      region:
        default: eu
security:
  - bearerAuth: []
paths:
  /pets/{petId}:
    parameters:
      - $ref: '#/components/parameters/PetId'
    get:
      summary: Find pet
      tags: [pets]
      parameters:
        - name: expand
          in: query
          schema: {type: string}
        - name: fields
          in: query
          required: true
          schema: {type: string, default: name}
        - name: X-Trace
          in: header
          required: true
      responses:
        200:
          description: The pet
    put:
      operationId: updatePet
      tags: [pets]
      requestBody:
        content:
          application/xml:
            schema: {$ref: '#/components/schemas/Pet'}
          application/json:
            schema: {$ref: '#/components/schemas/Pet'}
      responses:
        '200':
          description: Updated
  /health:
    get:
      security: []
      responses:
        '200':
          description: Healthy
    trace:
      summary: Trace health
  /stores/{storeId}/orders:
    post:
      tags: [store]
      security:
        - apiKey: []
      requestBody:
        content:
          application/json:
            example: {quantity: 2}
      responses:
        '201':
          description: Created
components:
  parameters:
    PetId:
      name: petId
      in: path
      required: true
      schema: {type: integer, example: 7}
  securitySchemes:
    bearerAuth: {type: http, scheme: bearer}
    apiKey: {type: apiKey, in: header, name: X-API-Key}
  schemas:
    Pet:
      type: object
      properties:
        id: {type: integer, readOnly: true}
        name: {type: string, example: Rex}
        tags:
          type: array
          items: {type: string}
        born: {type: string, format: date}
        owner: {$ref: '#/components/schemas/Owner'}
        status: {type: string, enum: [available, sold]}
    Owner:
      allOf:
        - type: object
          properties:
            name: {type: string}
        - type: object
          properties:
            pets:
              type: array
              items: {$ref: '#/components/schemas/Pet'}
"##;

#[test]
fn openapi_operations_are_grouped_by_tag_in_document_order() {
    let import = parse(PET_STORE).unwrap();
    let collection = &import.collection;

    assert_eq!(collection.name, "Pet Store");
    assert_eq!(collection.variables["base_url"], "https://eu.pets.test/v1");
    assert_eq!(collection.variables["bearerAuth"], "");
    assert_eq!(collection.variables["apiKey"], "");

    let (name, pets) = folder(&collection.items[0]);
    assert_eq!(name, "pets");
    assert_eq!(http(&pets[0]).0, "Find pet");
    assert_eq!(http(&pets[1]).0, "updatePet");
    assert_eq!(folder(&collection.items[1]).0, "store");

    // Operations without tags follow the folders.
    let (name, health) = http(&collection.items[2]);
    assert_eq!(name, "GET /health");
    assert!(health.headers.is_empty());

    assert_eq!(import.skipped, ["Trace health"]);
}

#[test]
fn openapi_parameters_fill_the_url_query_and_headers() {
    let import = parse(PET_STORE).unwrap();
    let (_, pets) = folder(&import.collection.items[0]);
    let (_, find) = http(&pets[0]);

    assert_eq!(find.method, Method::Get);
    assert_eq!(find.path, "{{base_url}}/pets/7");
    // Optional parameters are left out.
    assert_eq!(find.query, [("fields".to_owned(), "name".to_owned())]);
    assert_eq!(
        find.headers,
        [
            ("X-Trace".to_owned(), "{{X-Trace}}".to_owned()),
            (
                "Authorization".to_owned(),
                "Bearer {{bearerAuth}}".to_owned()
            ),
        ]
    );

    let (_, store) = folder(&import.collection.items[1]);
    let (_, order) = http(&store[0]);
    assert_eq!(order.path, "{{base_url}}/stores/{{storeId}}/orders");
    assert_eq!(
        order.headers,
        [
            ("X-API-Key".to_owned(), "{{apiKey}}".to_owned()),
            ("Content-Type".to_owned(), "application/json".to_owned()),
        ]
    );
    assert_eq!(json_body(order), json!({"quantity": 2}));
}

#[test]
fn openapi_bodies_are_generated_from_schemas() {
    let import = parse(PET_STORE).unwrap();
    let (_, pets) = folder(&import.collection.items[0]);
    let (_, update) = http(&pets[1]);

    assert_eq!(update.method, Method::Put);
    assert_eq!(
        update.headers[1],
        ("Content-Type".to_owned(), "application/json".to_owned())
    );

    let body = json_body(update);
    assert_eq!(body["name"], "Rex");
    assert_eq!(body["tags"], json!(["string"]));
    assert_eq!(body["born"], "2024-01-01");
    assert_eq!(body["status"], "available");
    assert_eq!(body["owner"]["name"], "string");
    assert!(body.get("id").is_none());
    // Recursive schemas stop instead of growing without end.
    assert!(body["owner"]["pets"][0]["owner"].is_object());
}

#[test]
fn swagger_specifications_are_imported() {
    let import = parse(
        r##"{
            "swagger": "2.0",
            "info": {"title": "Legacy"},
            "host": "legacy.test",
            "basePath": "/api",
            "schemes": ["http"],
            "consumes": ["application/json"],
            "securityDefinitions": {"key": {"type": "apiKey", "in": "query", "name": "api_key"}},
            "paths": {
                "/users/{id}": {
                    "post": {
                        "summary": "Update user",
                        "security": [{"key": []}],
                        "parameters": [
                            {"name": "id", "in": "path", "required": true, "type": "string"},
                            {"name": "user", "in": "body", "schema": {"$ref": "#/definitions/User"}}
                        ]
                    }
                },
                "/login": {
                    "post": {
                        "consumes": ["application/x-www-form-urlencoded"],
                        "parameters": [
                            {"name": "user", "in": "formData", "type": "string", "default": "admin"},
                            {"name": "avatar", "in": "formData", "type": "file"}
                        ]
                    }
                }
            },
            "definitions": {
                "User": {"type": "object", "properties": {"age": {"type": "integer"}}}
            }
        }"##,
    )
    .unwrap();
    let collection = &import.collection;

    assert_eq!(collection.variables["base_url"], "http://legacy.test/api");

    let (_, update) = http(&collection.items[0]);
    assert_eq!(update.path, "{{base_url}}/users/{{id}}");
    assert_eq!(update.query, [("api_key".to_owned(), "{{key}}".to_owned())]);
    assert_eq!(json_body(update), json!({"age": 0}));

    let (name, login) = http(&collection.items[1]);
    assert_eq!(name, "POST /login");
    assert_eq!(login.body.as_deref(), Some(b"user=admin".as_slice()));
}

#[test]
fn unsupported_openapi_versions_are_rejected() {
    let error = parse("swagger: '1.2'\ninfo: {title: Old}\n").err().unwrap();

    assert!(matches!(error, ImportError::UnsupportedOpenApi(version) if version == "1.2"));
}
