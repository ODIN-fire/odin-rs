# User Authentication and Authorization

ODIN servers can optionally authenticate users and authorize access to resources based on configurable user
roles. If a server config has no `auth` section (the default) everything works as before: all resources are public and
there is no runtime overhead.

The main design goals are:

- **password-free authentication** to minimize server administration. Users register their own credentials
  (passkeys or authenticator apps) with an enrollment link created by the administrator, who never handles user
  secrets
- support for **ad-hoc, incident specific networks and organization specific private LANs** that only have IP addresses
  (no DNS names), which rules out passkeys for such deployments and requires authenticator app (TOTP) support
- a **simple authorization model** based on up to 64 configurable roles. The primary protected resources are the
  application document and its websocket. Other protected resources (server routes and incoming websocket messages)
  are the exception and have to be explicitly marked
- **server type independence** - the core functions are in `odin_common::auth` so that they can be used by other
  server types than `odin_server::SpaServer`

## Concept

### What is protected

| resource | protection |
|----------|------------|
| application document (`/<app>`) | requires a valid session - without it the server returns a login document at the same URL |
| websocket (`/<app>/ws`) | requires a valid session and a same-origin request (otherwise 401/403) |
| proxied resources (`/<app>/proxy/...`) | require a valid session (unless `protect_proxies: false`) so that the server can't be used as an open proxy |
| explicitly protected routes | require a session user with any of the configured roles |
| incoming websocket messages | messages with configured role requirements are dropped if the user does not have the roles |
| assets (`/<app>/asset/...`) | **public** - assets must not contain sensitive data (see [client](client.md)) |
| other service routes (data, tiles etc.) | **public** unless added as protected routes |

Once a websocket connection is established its messages are not re-authenticated (except for messages that have role
requirements). Sessions of open connections are re-validated on each server heartbeat - connections of expired, revoked
or disabled users are closed, which sends the client back to the login document.

### Authentication methods

| method | requirements | use case |
|--------|--------------|----------|
| `Passkey` (WebAuthn) | secure context: `localhost` or HTTPS with a DNS name that matches `rp_id`. **Does not work with IP addresses** | primary method for demos (localhost) and servers with DNS names |
| `Totp` (authenticator apps, RFC 6238) | none (works with IP addresses). Should use TLS on non-loopback addresses | IP-only LANs, ad-hoc networks |
| `Password` | none. Should use TLS on non-loopback addresses | bootstrapping, fallback |

Passkeys use platform authenticators (Touch ID, Windows Hello, Android/iOS passkeys - which can be synced across
devices) or security keys. Authenticator apps are standard TOTP apps (Google/Microsoft Authenticator, 1Password etc.)
with 6 digit codes and 30 sec steps; each code can only be used once.

The login dialog only offers passkeys if the document location can use them (secure context and host name matching
`rp_id`), i.e. the same server can be used with passkeys via `http://localhost:9009` on the server machine and with
TOTP via `https://192.168.1.10:9009` from other machines.

### How sites, devices and tools work together

All methods share the same administration and enrollment flow. The administrator only creates users and sends them
an enrollment link, users register their credentials themselves:

```text
  server machine                                                            user
 ┌────────────────────────────────────────────────────┐
 │                                                    │   enrollment link    ┌──────────┐
 │  ┌────────────┐ --add-user alice --roles editor    │   (email, chat ..)   │   user   │
 │  │ auth_admin ├────────────────────────────────────┼─────────────────────►│          │
 │  └─────┬──────┘                                    │                      └────┬─────┘
 │        │ users, roles, invites                     │                           │ opens link
 │        ▼                                           │                           ▼
 │  ┌────────────┐      ┌──────────────────────────┐  │  /<app>/auth/enroll  ┌──────────┐
 │  │  auth.db   │◄────►│  SpaServer               │◄─┼──────────────────────┤ browser  │
 │  │  (SQLite)  │      │  enroll/info             │  │  register passkey,   │ auth.js  │
 │  └────────────┘      │  enroll/<method>/start   │  │  authenticator app   │          │
 │                      │  enroll/<method>/finish  ├──┼─────────────────────►│          │
 │                      └──────────────────────────┘  │  session cookie      └──────────┘
 └────────────────────────────────────────────────────┘  (logged in)
```

**Passkeys** - the private key never leaves the authenticator, the server only stores the public key. The browser
has to use `localhost` (on the server machine) or the configured DNS name (`rp_id`) over HTTPS:

```text
  user device                                             server machine
 ┌───────────────────────────────────────────────┐       ┌──────────────────────────────┐
 │                                               │       │  SpaServer                   │
 │  ┌──────────────────┐        ┌─────────────┐  │  (1)  │                              │
 │  │ authenticator    │        │ browser     ├──┼──────►│  login/passkey/start         │
 │  │                  │  (3)   │ auth.js     │  │  (2)  │    -> challenge              │
 │  │ Touch ID,        │◄───────┤             │◄─┼───────┤                              │
 │  │ Windows Hello,   │  get() │ http://     │  │       │                              │
 │  │ phone (QR/BLE),  │        │  localhost  │  │  (5)  │  login/passkey/finish        │
 │  │ security key     ├───────►│ https://    ├──┼──────►│    verify signature with     │
 │  │                  │  (4)   │  <rp_id>    │  │  (6)  │    stored public key         │
 │  └────────▲─────────┘ signed │             │◄─┼───────┤    -> session cookie         │
 │           │         challenge└─────────────┘  │       │             │                │
 └───────────┼───────────────────────────────────┘       │       ┌─────▼─────┐          │
             │ fingerprint, face or PIN                  │       │  auth.db  │          │
            user                                         │       │ public key│          │
                                                         │       └───────────┘          │
                                                         └──────────────────────────────┘
```

**Authenticator apps (TOTP)** - work with IP addresses and without internet access. The phone app and the server
share a secret (transferred once as QR code during enrollment) and compute codes from it and the current time, i.e.
there is no connection between phone and server, but **their clocks have to agree within about 30 sec**:

```text
                          incident LAN (IP addresses only)
                         ┌────────────────────────────────────────────────────────────────────┐
  user                   │  client machine                       server machine               │
 ┌────────────────────┐  │ ┌──────────────────────────┐          ┌────────────────────────┐   │
 │ phone              │  │ │ browser                  │  enroll  │  SpaServer             │   │
 │ ┌────────────────┐ │  │ │ https://192.168.1.10:9009│  start   │                        │   │
 │ │ authenticator  │◄┼──┼─┤  QR code (enrollment)    │◄─────────┤  enroll/totp/start     │   │
 │ │ app            │ │  │ │                          │          │    -> new secret       │   │
 │ │                │ │  │ │                          │  login   │                        │   │
 │ │ secret + clock │ │  │ │  user name + code        ├─────────►│  login/totp            │   │
 │ └───────┬────────┘ │  │ │                          │          │    code == HMAC(secret,│   │
 └─────────┼──────────┘  │ │                          │◄─────────┤    time step), unused  │   │
           │             │ └────────────▲─────────────┘  cookie  │            │           │   │
           │ 6 digit code│              │                        │      ┌─────▼──────┐    │   │
           └─────────────┼──────────────┘                        │      │  auth.db   │    │   │
             user types  │                                       │      │ secret,    │    │   │
                         │                                       │      │ last step  │    │   │
                         │                                       │      └────────────┘    │   │
                         │                                       └────────────────────────┘   │
                         └────────────────────────────────────────────────────────────────────┘
```

**Passwords** - simple, but users have to remember them and the server stores (argon2) password hashes. Should only
be used for bootstrapping or as fallback, and over TLS on non-loopback addresses:

```text
  user                     client machine                         server machine
            password      ┌──────────────────┐   login/password   ┌──────────────────────┐
  user  ─────────────────►│ browser          ├───────────────────►│  SpaServer           │
                          │ auth.js          │◄───────────────────┤   argon2 verify      │
                          └──────────────────┘   session cookie   │        │             │
                                                                  │  ┌─────▼──────┐      │
                                                                  │  │  auth.db   │      │
                                                                  │  │  pw hash   │      │
                                                                  │  └────────────┘      │
                                                                  └──────────────────────┘
```

A typical incident deployment combines the methods - passkeys for users on the server machine, authenticator apps
for users on LAN client machines:

```text
  incident LAN (IP addresses only, no internet required)
 ┌───────────────────────────────────────────────────────────────────────────────────────┐
 │  server machine                                  client machines                      │
 │ ┌─────────────────────────────────────────┐     ┌───────────────────────────────────┐ │
 │ │ ODIN server  0.0.0.0:9009               │     │ browser                           │ │
 │ │ TLS (self signed) or allow_insecure     │◄────┤ https://192.168.1.10:9009/live    │ │
 │ │ methods: [Passkey, Totp]                │     │ login with TOTP code              │ │
 │ │                                         │     └─────────────────▲─────────────────┘ │
 │ │ auth_admin ──► auth.db                  │                       │ code              │
 │ │                                         │     ┌─────────────────┴─────────────────┐ │
 │ │ browser http://localhost:9009/live      │     │ phone with authenticator app      │ │
 │ │ login with passkey (Touch ID ..)        │     │ (clock in sync with server)       │ │
 │ └─────────────────────────────────────────┘     └───────────────────────────────────┘ │
 └───────────────────────────────────────────────────────────────────────────────────────┘
```

### Roles

Roles are configured as a list of up to 64 names (e.g. `["viewer","editor","admin"]`). Users have a set of roles,
resources require a set of roles. A user is authorized if it has **any** of the required roles. An empty set of
required roles means "any authenticated user". There is no role hierarchy - if `admin` should also be allowed to edit,
resources requiring `editor` have to list both (`["editor","admin"]`).

Internally role sets are bitmasks (`RoleSet(u64)`), i.e. authorization checks are a single bitwise AND. User roles are
stored by name, i.e. the configured role list can be reordered or extended without affecting stored users (roles that
are removed from the config are ignored).

### User lifecycle

Users are managed with the `auth_admin` tool, which works directly on the user database (also while servers are
running - changes take effect within 30 sec):

```shell
# create user and print enrollment link (valid for 'invite_ttl', default 24h)
auth_admin --add-user alice --roles editor --app odin_live [--expires-days 90]

# new enrollment link for an existing user (e.g. lost device)
auth_admin --invite alice --app odin_live

auth_admin --list                         # users, roles, status, credentials, active sessions
auth_admin --set-roles alice --roles editor,admin
auth_admin --revoke alice                 # end all sessions (connections are closed on next heartbeat)
auth_admin --disable alice                # also ends all sessions
auth_admin --enable alice
auth_admin --remove-credentials alice [--kind passkey|totp|password]
auth_admin --remove-user alice
auth_admin --set-password admin           # bootstrap (requires 'Password' method), reads password from stdin
```

The enrollment link (`<url>/<app>/auth/enroll?token=...`) is sent to the user (e.g. by email or chat). Opening it shows
a dialog to register a passkey, an authenticator app (QR code) or a password. Registering the first credential consumes
the token and logs the user in. Logged in users can add more credentials (e.g. a passkey on a second device or a backup
authenticator app) from the *UI Settings* window, which also has the logout button.

The enrollment URL base is derived from the server config (`localhost` for loopback addresses). Use `--url` if users
reach the server under a different address (e.g. `--url https://192.168.1.10:9009`).

## Configuration

Authentication is enabled by adding an `auth` section to the server config (e.g.
`$ODIN_ROOT/configs/odin_server/spa_server.ron`). Only `roles` is required:

```rust,ignore
ServerConfig(
    sock_addr: "127.0.0.1:9009",
    tls: None,
    heartbeat_interval: Some( Duration( secs: 60, nanos: 0) ),
    auth: Some( AuthConfig(
        roles: ["viewer", "editor", "admin"],
        methods: [Passkey, Totp],      // default, in order offered in the login dialog
        rp_id: "localhost",            // WebAuthn relying party (DNS name or "localhost"), default "localhost"
        rp_name: "ODIN",               // shown in passkey dialogs and authenticator apps
        session_ttl: "12h",            // max session duration
        invite_ttl: "24h",             // validity of enrollment links
        db_path: None,                 // default $ODIN_ROOT/data/auth/auth.db
        route_roles: { "share": ["editor","admin"] },  // overrides for protected routes
        ws_msg_roles: { "odin_share/odin_share.js:setShared": ["editor","admin"] }, // overrides for ws messages
        protect_proxies: true,         // default
        allow_insecure: false,         // allow plain HTTP on non-loopback addresses
        max_failures: 5,               // failed logins before lockout (per user name and per client address)
        lockout: "30s",                // initial lockout, doubled for each further failure (max 1h)
        min_password_len: 12,
    ))
)
```

The server refuses to start if

- the auth config is inconsistent (unknown role names, IP address as `rp_id` with passkeys enabled)
- authentication would be done over plain HTTP on a non-loopback address without `allow_insecure: true`
- `odin_server` was built without the `auth` feature (default)
- a service requires a websocket message role that is not configured (and not overridden in `ws_msg_roles`)

The `heartbeat_interval` determines how fast revoked sessions and role changes affect open connections.

### Deployment scenarios

- **demo on the server machine**: `sock_addr: "127.0.0.1:9009"`, `methods: [Passkey]`. Open
  `http://localhost:9009/<app>` (not `127.0.0.1` - passkeys need a host name). The server prints and opens the
  `localhost` URL if `rp_id` is "localhost"
- **IP-only LAN / ad-hoc network**: `sock_addr: "0.0.0.0:9009"`, `methods: [Totp]` (or `[Passkey, Totp]` so that
  users on the server machine can use passkeys via localhost). Use TLS with a self-signed certificate (`tls` section),
  or `allow_insecure: true` for isolated networks (TOTP codes and session cookies are then sent unencrypted)
- **server with DNS name**: `rp_id: "odin.example.org"`, TLS with a certificate for that name, `methods: [Passkey, Totp]`

## Authorization in Services

### Protected routes

Service routes are public unless they are added with `SpaComponents::add_protected_route(..)`:

```rust
fn add_components (&self, spa: &mut SpaComponents) -> OdinServerResult<()> {
    ...
    // default roles ["editor"] can be overridden with the 'route_roles' config key "my_upload"
    spa.add_protected_route( "my_upload", &["editor"], |router, state| {
        router.route( &format!("/{}/my-upload", state.name), post( upload_handler))
    });
    Ok(())
}

async fn upload_handler (Extension(user): Extension<Arc<ConnUser>>, body: Bytes) -> Response {...}
```

The route function receives an empty router and should only add the routes to protect. Handlers can obtain the
authenticated user with `Extension<Arc<ConnUser>>`. If the server does not use authentication protected routes are
added as normal routes (handlers that use the `Extension` extractor then fail, use `Option<Extension<..>>` if the
service should also work without authentication).

### Websocket messages

Services declare role requirements for incoming websocket messages by overriding `SpaService::ws_msg_roles()`:

```rust
impl SpaService for ShareService {
    ...
    fn ws_msg_roles (&self) -> Vec<(String,Vec<String>)> {
        vec![ (format!("{}:setShared", Self::mod_path()), vec!["editor".into(), "admin".into()]) ]
    }
}
```

Keys are `"<mod_path>:<msg_type>"` or `"<mod_path>"` for all messages of a module. The `ws_msg_roles` config can
override the defaults (and add requirements for services that do not declare any). Messages from users without the
required roles are not passed to `SpaService::handle_ws_msg(..)` - the client receives a `__system__` *denied*
message instead.

Since `SpaService::init_connection(..)` receives the `WsConnection`, which includes the authenticated user
(`conn.user: Option<Arc<ConnUser>>`, `None` if the server does not use authentication), services can also send role
dependent initial data (`conn.is_authorized(roles)`).

### Client side

See [client](client.md) for the `ui.js` functions `isUserAuthorized(role)`, `getUserName()` etc. These are only for
the user interface - authorization is always enforced by the server.

## Implementation

### Components

| component | purpose |
|-----------|---------|
| `odin_common::auth` (module root) | `RoleSet`, `RoleMap`, `ConnUser`, `MsgAcl`, `AuthConfig` - always compiled since `WsConnection` refers to `ConnUser` |
| `odin_common::auth::AuthStore` | SQLite user database (users, credentials, invites, sessions) - `auth` feature |
| `odin_common::auth::Authenticator` | sessions, login, enrollment, user administration, rate limiting - `auth` feature |
| `odin_server::auth` | axum integration: `ServerAuth` (session cookies), `auth_router()` (JSON API), `protect()` (role middleware), `is_same_origin()` |
| `odin_server::spa::SpaServer` | document/login document, websocket gate, protected routes and proxies, ws message ACL, heartbeat re-validation |
| `odin_server/assets/auth.js` | client side protocol (login, enrollment, logout, WebAuthn JSON conversion) |
| `odin_server/assets/ui_dialogs.js` | `loginDialog`, `enrollDialog`, `totpSetupDialog`, `newPasswordDialog` |
| `odin_server/assets/ui.js` | `isUserAuthorized()` etc. |
| `odin_server/assets/ws.js` | `__system__` messages, return to login document if the session ended |
| `auth_admin` | user administration tool |

The `auth` Cargo feature of `odin_server` (default) enables `odin_common/auth`, which adds the SQLite (`sqlx`),
WebAuthn (`webauthn-rs`, which uses OpenSSL), TOTP (`totp-rs`) and password hashing (`password-auth`, argon2)
dependencies. Other server types can use `odin_common::auth::Authenticator` directly, or `odin_server::auth` if they
are axum based.

### Document flow

```
browser                                     SpaServer
   |  GET /<app>  (no session cookie)          |
   |------------------------------------------>|  login document: theme + auth.js, preloads all local assets
   |  POST auth/login/{passkey|totp|password}  |
   |------------------------------------------>|  Set-Cookie: odin_session_<port>=<token>; HttpOnly; SameSite=Strict
   |  GET /<app>  (reload with cookie)         |
   |------------------------------------------>|  application document with window.odinUser={name,roles}
   |  GET /<app>/ws  (cookie, same origin)     |
   |------------------------------------------>|  101 - connection with ConnUser
```

The login document is generated from the same components as the application document (`SpaComponents::to_auth_html`).
It executes only the theme script and the `auth.js` module (which imports `ui.js` and `ui_dialogs.js`), all other
local assets are preloaded. Assets are served with `ETag` and `Cache-Control: no-cache`, i.e. the application
document can use the preloaded assets after a revalidation request.

Passkey login and enrollment are two step *ceremonies*: a `start` request returns a challenge (the options for
`navigator.credentials.get()/create()`), the `finish` request verifies the signed response. Pending ceremonies are kept
in memory for 5 minutes. TOTP enrollment works the same way: `start` returns a new secret as QR code, `finish` verifies
the first code (wrong codes can be retried).

### Sessions

Session tokens are 256 bit random values stored in an `HttpOnly`, `SameSite=Strict` cookie (`Secure` if TLS is
used). The cookie name includes the server port (`odin_session_9009`) since cookies are not port specific. A session is
valid for all applications served by the same server.

The database only stores the SHA-256 hash of the token, which is also used as session id (`ConnUser::session`). Sessions
are persistent, i.e. users stay logged in across server restarts (until `session_ttl`). Each user has an *epoch*
counter that is stored with its sessions - incrementing it (`--revoke`, `--disable`, `--remove-credentials`)
invalidates all sessions of the user.

Session and user lookups are cached in memory. The user cache is reloaded every 30 sec and on each heartbeat, when
open connections are also re-validated. Connections with invalid sessions are closed, connections of users with changed
roles are updated (and the client is notified so that `ui.js` can update its user info).

### Security measures

- **no secrets in clear**: session and invite tokens are stored as SHA-256 hashes, passwords as argon2 hashes.
  TOTP secrets have to be stored as they are, hence the database is created with owner-only permissions (0600)
- **CSRF**: `SameSite=Strict` cookies and JSON-only POST requests (forms can't send `application/json`)
- **cross site websocket hijacking**: websocket upgrades have to come from the same origin
- **brute force**: failed logins lock the user name and the client address after `max_failures` attempts, with
  exponentially increasing lockout durations (max 1h). Login with unknown user names takes the same time as with
  known ones (dummy password verification)
- **TOTP replay**: each code (time step) can only be used once per credential
- **passkeys** require user verification, signature counters are updated on each login
- **transport**: the server refuses to authenticate over plain HTTP on non-loopback addresses unless explicitly allowed
- **open proxy**: proxied URLs require a session
- **enrollment tokens** are single use, expire after `invite_ttl` and are removed from the browser URL/history

### Testing

- `odin_common/tests/test_auth.rs` - sessions, revocation, role changes, lockout, TOTP (incl. replay), passkey
  registration/authentication with a software authenticator (`webauthn-authenticator-rs`)
- `odin_server/tests/test_auth.rs` - JSON API, cookies, protected routes, origin check, insecure config detection
- `odin_live` built with the `auth-passkey`, `auth-totp` and/or `auth-pw` features serves as integration test for
  production servers (most ODIN services with authentication)

The browser side was tested end-to-end with headless Chrome and a CDP virtual authenticator (passkey enrollment and
login, logout, revocation, password login, role change notification, TOTP enrollment and login).

## Limitations and Possible Extensions

- passkey login requires the user name (no discoverable credential / conditional UI login yet)
- no self-service account recovery - a lost device requires a new enrollment link from the administrator
- no web based user administration (use `auth_admin`)
- authorization is per message type, not per data item (e.g. no per-layer data filtering)
- outgoing (broadcast) websocket messages are not filtered by roles
