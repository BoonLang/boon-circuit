use boon_runtime::{
    ApplicationIdentity, ProgramCapabilityProfile, ProgramCompileRequest, RuntimeSourceUnit,
    compile_program_artifact,
};
use boon_server_host::{ServerConfig, bind};
use boon_server_runtime::BoonServerProgram;
use boon_transport_json::{Limits, Value, decode};
use reqwest::{Client, Method, Response, StatusCode};
use std::{
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
};

const SHARED_PATH: &str = "examples/fjordpulse/Shared/FjordPulseContract.bn";
const SERVER_PATH: &str = "examples/fjordpulse/Server/RUN.bn";

fn target_http_fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/fjordpulse/contracts/target/fixtures/http")
        .join(name)
}

fn loopback() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
}

fn compile_server() -> BoonServerProgram {
    let artifact = compile_program_artifact(&ProgramCompileRequest {
        revision: 1,
        entry_path: SERVER_PATH.to_owned(),
        units: vec![
            RuntimeSourceUnit {
                path: SHARED_PATH.to_owned(),
                source: include_str!("../../../examples/fjordpulse/Shared/FjordPulseContract.bn")
                    .to_owned(),
            },
            RuntimeSourceUnit {
                path: SERVER_PATH.to_owned(),
                source: include_str!("../../../examples/fjordpulse/Server/RUN.bn").to_owned(),
            },
        ],
        application: ApplicationIdentity::new(
            "cz.kavik.fjordpulse",
            "server-contract-test",
            "loopback",
        ),
        capability_profile: ProgramCapabilityProfile::TrustedServer,
    })
    .expect("FjordPulse deterministic Server should compile");

    assert!(
        artifact
            .plan()
            .capability_summary
            .cpu_plan_executor_complete
    );
    assert_eq!(
        artifact.plan().query_indexes.len(),
        1,
        "station search must remain compiler-indexed"
    );
    BoonServerProgram::new(artifact).expect("generic HTTP host port should resolve")
}

async fn response_json(response: Response) -> (StatusCode, Value) {
    let status = response.status();
    let body = response.bytes().await.expect("response body should read");
    let value = decode(&body, &Limits::STRICT_SERVER_CLIENT)
        .unwrap_or_else(|error| panic!("response must be bounded canonical JSON: {error}"));
    (status, value)
}

fn fixture(bytes: &[u8]) -> Value {
    decode(bytes, &Limits::STRICT_SERVER_CLIENT).expect("committed target fixture must be valid")
}

async fn request(client: &Client, address: SocketAddr, method: Method, path: &str) -> Response {
    client
        .request(method, format!("http://{address}{path}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("{path} loopback request failed: {error}"))
}

async fn assert_fixture(
    client: &Client,
    address: SocketAddr,
    method: Method,
    path: &str,
    status: StatusCode,
    expected: &[u8],
) {
    let (actual_status, actual) = response_json(request(client, address, method, path).await).await;
    assert_eq!(actual_status, status, "status mismatch for {path}");
    assert_eq!(actual, fixture(expected), "fixture mismatch for {path}");
}

async fn write_target_fixture(
    client: &Client,
    address: SocketAddr,
    method: Method,
    path: &str,
    status: StatusCode,
    fixture_name: &str,
) {
    let response = request(client, address, method, path).await;
    assert_eq!(response.status(), status, "status mismatch for {path}");
    let body = response.bytes().await.expect("response body should read");
    decode(&body, &Limits::STRICT_SERVER_CLIENT)
        .unwrap_or_else(|error| panic!("{path} must return bounded canonical JSON: {error}"));

    let fixture_path = target_http_fixture_path(fixture_name);
    fs::create_dir_all(
        fixture_path
            .parent()
            .expect("target fixture should have a parent directory"),
    )
    .expect("target fixture directory should be creatable");
    fs::write(&fixture_path, &body)
        .unwrap_or_else(|error| panic!("{} should be writable: {error}", fixture_path.display()));
}

fn error_code(value: &Value) -> &str {
    let Value::Record(root) = value else {
        panic!("error response root must be a record")
    };
    let Value::Record(error) = &root["error"] else {
        panic!("error response must contain an error record")
    };
    let Value::Text(code) = &error["code"] else {
        panic!("error code must be Text")
    };
    code
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fjordpulse_public_http_slice_matches_committed_target_contracts() {
    let server = bind(loopback(), ServerConfig::default(), compile_server())
        .await
        .expect("generic loopback server should bind");
    let address = server.local_addr();
    let client = Client::new();

    assert_fixture(
        &client,
        address,
        Method::GET,
        "/api/health",
        StatusCode::OK,
        include_bytes!(
            "../../../examples/fjordpulse/contracts/target/fixtures/http/health-response.json"
        ),
    )
    .await;
    assert_fixture(
        &client,
        address,
        Method::GET,
        "/api/readiness",
        StatusCode::OK,
        include_bytes!(
            "../../../examples/fjordpulse/contracts/target/fixtures/http/readiness-response.json"
        ),
    )
    .await;
    assert_fixture(
        &client,
        address,
        Method::GET,
        "/api/map/config",
        StatusCode::OK,
        include_bytes!(
            "../../../examples/fjordpulse/contracts/target/fixtures/http/map-config-response.json"
        ),
    )
    .await;
    assert_fixture(
        &client,
        address,
        Method::GET,
        "/api/stations",
        StatusCode::OK,
        include_bytes!(
            "../../../examples/fjordpulse/contracts/target/fixtures/http/station-map-response.json"
        ),
    )
    .await;
    assert_fixture(
        &client,
        address,
        Method::GET,
        "/api/search?q=f%C3%B8rde",
        StatusCode::OK,
        include_bytes!(
            "../../../examples/fjordpulse/contracts/target/fixtures/http/search-response.json"
        ),
    )
    .await;
    assert_fixture(
        &client,
        address,
        Method::GET,
        "/api/search?q=ber",
        StatusCode::OK,
        include_bytes!(
            "../../../examples/fjordpulse/contracts/target/fixtures/http/search-bergen-response.json"
        ),
    )
    .await;
    assert_fixture(
        &client,
        address,
        Method::GET,
        "/api/stations/NSR:StopPlace:548",
        StatusCode::OK,
        include_bytes!(
            "../../../examples/fjordpulse/contracts/target/fixtures/http/station-response.json"
        ),
    )
    .await;
    assert_fixture(
        &client,
        address,
        Method::GET,
        "/api/stations/NSR:StopPlace:548/departures",
        StatusCode::OK,
        include_bytes!(
            "../../../examples/fjordpulse/contracts/target/fixtures/http/station-departures-response.json"
        ),
    )
    .await;
    assert_fixture(
        &client,
        address,
        Method::GET,
        "/api/stations/NSR:StopPlace:548/nearby-vehicles",
        StatusCode::OK,
        include_bytes!(
            "../../../examples/fjordpulse/contracts/target/fixtures/http/nearby-vehicles-response.json"
        ),
    )
    .await;
    assert_fixture(
        &client,
        address,
        Method::GET,
        "/api/vehicles/SKY:Vehicle:12345",
        StatusCode::OK,
        include_bytes!(
            "../../../examples/fjordpulse/contracts/target/fixtures/http/vehicle-response.json"
        ),
    )
    .await;
    assert_fixture(
        &client,
        address,
        Method::POST,
        "/api/realtime-token",
        StatusCode::CREATED,
        include_bytes!(
            "../../../examples/fjordpulse/contracts/target/fixtures/http/realtime-token-response.json"
        ),
    )
    .await;

    server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicitly refreshes source-controlled FjordPulse target fixtures"]
async fn regenerate_fjordpulse_target_http_fixtures_from_server_outputs() {
    let server = bind(loopback(), ServerConfig::default(), compile_server())
        .await
        .expect("generic loopback server should bind");
    let address = server.local_addr();
    let client = Client::new();

    let fixtures = [
        (
            Method::GET,
            "/api/health",
            StatusCode::OK,
            "health-response.json",
        ),
        (
            Method::GET,
            "/api/readiness",
            StatusCode::OK,
            "readiness-response.json",
        ),
        (
            Method::GET,
            "/api/map/config",
            StatusCode::OK,
            "map-config-response.json",
        ),
        (
            Method::GET,
            "/api/stations",
            StatusCode::OK,
            "station-map-response.json",
        ),
        (
            Method::GET,
            "/api/search?q=f%C3%B8rde",
            StatusCode::OK,
            "search-response.json",
        ),
        (
            Method::GET,
            "/api/search?q=ber",
            StatusCode::OK,
            "search-bergen-response.json",
        ),
        (
            Method::GET,
            "/api/stations/NSR:StopPlace:548",
            StatusCode::OK,
            "station-response.json",
        ),
        (
            Method::GET,
            "/api/stations/NSR:StopPlace:548/departures",
            StatusCode::OK,
            "station-departures-response.json",
        ),
        (
            Method::GET,
            "/api/stations/NSR:StopPlace:548/nearby-vehicles",
            StatusCode::OK,
            "nearby-vehicles-response.json",
        ),
        (
            Method::GET,
            "/api/vehicles/SKY:Vehicle:12345",
            StatusCode::OK,
            "vehicle-response.json",
        ),
        (
            Method::POST,
            "/api/realtime-token",
            StatusCode::CREATED,
            "realtime-token-response.json",
        ),
    ];

    for (method, path, status, fixture_name) in fixtures {
        write_target_fixture(&client, address, method, path, status, fixture_name).await;
    }

    server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fjordpulse_public_http_slice_rejects_invalid_method_query_and_ids() {
    let server = bind(loopback(), ServerConfig::default(), compile_server())
        .await
        .expect("generic loopback server should bind");
    let address = server.local_addr();
    let client = Client::new();

    let cases = [
        (
            Method::POST,
            "/api/health",
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
        ),
        (
            Method::GET,
            "/api/search",
            StatusCode::BAD_REQUEST,
            "invalid_query",
        ),
        (
            Method::GET,
            "/api/search?q=one&q=two",
            StatusCode::BAD_REQUEST,
            "invalid_query",
        ),
        (
            Method::GET,
            "/api/stations/not-an-id",
            StatusCode::BAD_REQUEST,
            "invalid_station",
        ),
        (
            Method::GET,
            "/api/stations/NSR:StopPlace:999",
            StatusCode::NOT_FOUND,
            "station_not_found",
        ),
        (
            Method::GET,
            "/api/vehicles/not-an-id",
            StatusCode::BAD_REQUEST,
            "invalid_vehicle",
        ),
        (
            Method::GET,
            "/api/vehicles/SKY:Vehicle:999",
            StatusCode::NOT_FOUND,
            "vehicle_not_found",
        ),
        (
            Method::GET,
            "/api/does-not-exist",
            StatusCode::NOT_FOUND,
            "route_not_found",
        ),
    ];
    for (method, path, expected_status, expected_code) in cases {
        let (status, value) = response_json(request(&client, address, method, path).await).await;
        assert_eq!(status, expected_status, "status mismatch for {path}");
        assert_eq!(
            error_code(&value),
            expected_code,
            "error mismatch for {path}"
        );
    }

    server.shutdown().await.unwrap();
}
