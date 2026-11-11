//! Domain errors for the `github-publish` plugin.

use thiserror::Error;

/// Errors that can occur during invocation and execution of `github-publish`.
#[derive(Debug, Error)]
pub enum PluginError {
    /// The input was not a valid `PluginInvocation` envelope.
    #[error("invalid invocation envelope: {0}")]
    Envelope(#[source] serde_json::Error),

    /// The envelope named a capability this plugin does not implement.
    #[error("unsupported capability `{capability}`")]
    UnsupportedCapability { capability: String },

    /// The envelope named an operation this plugin does not implement.
    #[error("unsupported operation `{operation}`")]
    UnsupportedOperation { operation: String },

    /// The payload could not be decoded.
    #[error("invalid payload for operation `{operation}`: {source}")]
    Payload {
        operation: String,
        #[source]
        source: serde_json::Error,
    },

    /// The response could not be encoded.
    #[error("failed to encode response: {0}")]
    Response(#[source] serde_json::Error),

    /// Authentication token is missing.
    #[error(
        "missing GitHub token: neither GITHUB_TOKEN nor GH_TOKEN is configured in plugin environment"
    )]
    MissingToken,

    /// Repository identifier (`owner/repo`) could not be determined.
    #[error(
        "could not determine GitHub repository: specify it in configuration or via GITHUB_REPOSITORY environment variable"
    )]
    MissingRepository,

    /// Invalid repository format.
    #[error("invalid repository format `{0}`: expected `owner/repo`")]
    InvalidRepository(String),

    /// GitHub REST API returned an error response.
    #[error("GitHub API error: HTTP {status} from {endpoint}: {message}")]
    GitHubApi {
        status: u16,
        endpoint: String,
        message: String,
    },

    /// An asset could not be read.
    #[error("failed to read asset at `{path}`: {reason}")]
    AssetRead { path: String, reason: String },

    /// Error making HTTP request.
    #[error("HTTP request to `{url}` failed: {reason}")]
    Http { url: String, reason: String },

    /// Configuration error.
    #[error("configuration error: {0}")]
    Config(String),
}
