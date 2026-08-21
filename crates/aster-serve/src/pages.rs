//! The states a browser can land on that are not the app: a build with no UI,
//! and the guard's refusals. One shell, so they read as one product.

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

const SHELL: &str = include_str!("page.html");

pub fn render(status: StatusCode, body: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        SHELL.replace("<!-- BODY -->", body),
    )
        .into_response()
}

/// What a browser gets when the binary was built without a UI bundle: the
/// command that fixes it, rather than a blank page.
pub fn missing_ui() -> Response {
    render(StatusCode::OK, include_str!("no-ui.html"))
}

/// The token server refusing a bare visit: the URL in the terminal is the key.
pub fn needs_token() -> Response {
    render(
        StatusCode::FORBIDDEN,
        r#"<h1>This server wants its token.</h1>
      <p>
        Aster is listening beyond this machine, so the URL carries a secret and
        this visit arrived without it.
      </p>
      <p class="note">
        Open the full URL printed by the terminal running <b>aster serve</b>;
        it looks like <code>http://host:4187/?token=…</code> and only needs to
        be used once per browser.
      </p>"#,
    )
}

/// A loopback server reached through a name it does not serve: most likely a
/// DNS-rebinding page, occasionally a proxy in the way.
pub fn wrong_address() -> Response {
    render(
        StatusCode::FORBIDDEN,
        r#"<h1>This address is not the one Aster serves.</h1>
      <p>
        The request reached Aster through a name it does not answer to, which
        is how a malicious page would try to reach a local agent. Nothing was
        run.
      </p>
      <p class="note">
        Open the address the terminal printed, e.g.
        <code>http://localhost:4187/</code>.
      </p>"#,
    )
}
