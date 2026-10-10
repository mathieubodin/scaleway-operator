//! Fausses réponses de l'API Scaleway, servies par mockito.

/// Réponse JSON 200 sur un chemin exact.
pub fn mock_json(
    server: &mut mockito::Server,
    method: &str,
    path: &str,
    body: &str,
) -> mockito::Mock {
    server
        .mock(method, path)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
}
