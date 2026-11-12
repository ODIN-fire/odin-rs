/*
 * Copyright 2026 Peter Mehlitz
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://apache.org
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

// client side of the odin_server authentication protocol (see odin_server/src/auth.rs). This module is executed by
// the login and enrollment documents served by SpaServer instances with authentication, and imported by ui_settings.js
// for logout and adding credentials. All requests use URLs relative to the application document (`auth/...`).
//
// login and enroll documents call `login(config)` or `enroll(config)` with the server generated config
// { methods: [..], rpId: "..", minPasswordLength: N }

import * as ui from "./ui.js";  // also preloads ui.js for the application document
import * as dialogs from "./ui_dialogs.js";

/// login loop of the login document. Reloads the (application) document once we have a session
export async function login (config) {
    let methods = usableMethods( config);
    let username = localStorage.getItem("odin-user") || "";
    let error = methods.length ? undefined : "no usable authentication method (passkeys require 'localhost' or HTTPS with the configured host name)";

    while (true) {
        let res = await dialogs.loginDialog({ methods, username, error, title: `Login to ${appName()}` });
        if (!res.method) { error = undefined; continue; } // dialog was force-closed
        username = res.username;
        if (!username) { error = "please enter user name"; continue; }

        try {
            if (res.method == "passkey") await passkeyLogin( username);
            else if (res.method == "totp") await postOk( "auth/login/totp", { username, code: res.secret });
            else if (res.method == "password") await postOk( "auth/login/password", { username, password: res.secret });

            localStorage.setItem("odin-user", username);
            location.reload(); // now we get the application document
            return;
        } catch (e) {
            error = errorText( e);
        }
    }
}

/// enrollment loop of the enrollment document (opened with an admin generated invite URL `auth/enroll?token=..`).
/// Goes to the application document once a credential has been registered
export async function enroll (config) {
    let url = new URL( location.href);
    let token = url.searchParams.get("token");
    history.replaceState( null, "", url.pathname); // don't keep the token in the browser history

    let info;
    try {
        info = await postOk( "auth/enroll/info", { token });
    } catch (e) {
        await dialogs.alert( errorText( e) + "\n\nplease ask your administrator for a new enrollment link", "Enrollment");
        location.href = appUrl();
        return;
    }

    let methods = usableMethods( info);
    let error = undefined;
    while (true) {
        let method = await dialogs.enrollDialog({ user: info.user, methods, error });
        try {
            if (await enrollMethod( method, token, info)) {
                localStorage.setItem("odin-user", info.user);
                location.href = appUrl(); // we are logged in now
                return;
            }
            error = undefined; // cancelled sub-dialog - choose again
        } catch (e) {
            error = errorText( e);
        }
    }
}

/// register an additional credential for the logged in user (e.g. a passkey on another device or a backup TOTP app)
export async function addCredential () {
    let info = await postOk( "auth/enroll/info", {});
    let methods = usableMethods( info);
    let error = undefined;
    while (true) {
        let method = await dialogs.enrollDialog({ user: info.user, methods, error, cancelable: true, title: "Add Credential" });
        if (!method) return false;
        try {
            if (await enrollMethod( method, null, info)) {
                await dialogs.alert( `${method} credential added for user '${info.user}'`, "Add Credential");
                return true;
            }
            error = undefined;
        } catch (e) {
            error = errorText( e);
        }
    }
}

/// end the session and go back to the login document
export async function logout () {
    try { await post( "auth/logout", {}); } catch (e) { console.log("logout failed: ", e); }
    location.href = appUrl();
}

//--- internal functions

function appName () {
    return document.baseURI.replace(/\/$/, "").split("/").pop();
}

function appUrl () {
    return document.baseURI.replace(/\/$/, ""); // base is "/<app>/", the document route is "/<app>"
}

/// methods that can be used from this document location. Passkeys require a secure context and a host that
/// matches the WebAuthn relying party id (passkeys do not work with IP addresses)
function usableMethods (config) {
    return (config.methods || []).filter( m => m != "passkey" || isPasskeyUsable( config.rpId));
}

function isPasskeyUsable (rpId) {
    let host = location.hostname;
    return window.isSecureContext && !!window.PublicKeyCredential && rpId && (host == rpId || host.endsWith("." + rpId));
}

async function enrollMethod (method, token, info) {
    if (method == "passkey") {
        let label = await dialogs.prompt( "name of this passkey (e.g. device):", defaultDeviceName(), "Passkey");
        if (label === null) return false;
        let start = await postOk( "auth/enroll/passkey/start", { token, label });
        let cred = await navigator.credentials.create( { publicKey: creationOptions( start.options.publicKey) });
        if (!cred) return false;
        await postOk( "auth/enroll/passkey/finish", { ceremony: start.ceremony, credential: registrationToJson( cred) });
        return true;

    } else if (method == "totp") {
        let start = await postOk( "auth/enroll/totp/start", { token });
        let error = undefined;
        while (true) {
            let code = await dialogs.totpSetupDialog( { qrPng: start.qrPng, secret: start.secret, error });
            if (code === null) return false;
            try {
                await postOk( "auth/enroll/totp/finish", { ceremony: start.ceremony, code, label: "totp" });
                return true;
            } catch (e) {
                if (e.status == 401) { error = "wrong code, please try again"; continue; } // same ceremony can be retried
                throw e;
            }
        }

    } else if (method == "password") {
        let password = await dialogs.newPasswordDialog( { minLength: info.minPasswordLength });
        if (password === null) return false;
        await postOk( "auth/enroll/password", { token, password });
        return true;
    }
    return false;
}

function defaultDeviceName () {
    let ua = navigator.userAgent;
    let os = /Mac/.test(ua) ? "Mac" : /iPhone|iPad/.test(ua) ? "iOS" : /Android/.test(ua) ? "Android" : /Windows/.test(ua) ? "Windows" : /Linux/.test(ua) ? "Linux" : "device";
    let browser = /Edg\//.test(ua) ? "Edge" : /Chrome\//.test(ua) ? "Chrome" : /Firefox\//.test(ua) ? "Firefox" : /Safari\//.test(ua) ? "Safari" : "";
    return browser ? `${os} ${browser}` : os;
}

async function passkeyLogin (username) {
    let start = await postOk( "auth/login/passkey/start", { username });
    let cred = await navigator.credentials.get( { publicKey: requestOptions( start.options.publicKey) });
    if (!cred) throw new Error("no passkey selected");
    await postOk( "auth/login/passkey/finish", { ceremony: start.ceremony, credential: assertionToJson( cred) });
}

async function post (path, data) {
    return fetch( path, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        credentials: "same-origin",
        cache: "no-store",
        body: JSON.stringify( data)
    });
}

/// POST JSON and return the JSON response, throw an error with status and server message if not ok
async function postOk (path, data) {
    let response = await post( path, data);
    let json = {};
    try { json = await response.json(); } catch (e) {}
    if (!response.ok) {
        let err = new Error( json.error || `request failed (${response.status})`);
        err.status = response.status;
        throw err;
    }
    return json;
}

function errorText (e) {
    if (e.name == "NotAllowedError") return "passkey operation was cancelled or timed out";
    if (e.name == "InvalidStateError") return "this authenticator is already registered";
    return e.message || String(e);
}

//--- WebAuthn JSON conversion (the server uses base64url encoded binary fields)

function b64urlToBuf (s) {
    s = s.replace(/-/g, "+").replace(/_/g, "/");
    while (s.length % 4) s += "=";
    let bin = atob(s);
    let buf = new Uint8Array( bin.length);
    for (let i = 0; i < bin.length; i++) buf[i] = bin.charCodeAt(i);
    return buf.buffer;
}

function bufToB64url (buf) {
    let bytes = new Uint8Array( buf);
    let bin = "";
    for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode( bytes[i]);
    return btoa( bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function creationOptions (pk) {
    let o = structuredClone( pk);
    o.challenge = b64urlToBuf( pk.challenge);
    o.user.id = b64urlToBuf( pk.user.id);
    if (pk.excludeCredentials) o.excludeCredentials = pk.excludeCredentials.map( c => ({ ...c, id: b64urlToBuf( c.id) }));
    return o;
}

function requestOptions (pk) {
    let o = structuredClone( pk);
    o.challenge = b64urlToBuf( pk.challenge);
    if (pk.allowCredentials) o.allowCredentials = pk.allowCredentials.map( c => ({ ...c, id: b64urlToBuf( c.id) }));
    return o;
}

function registrationToJson (cred) {
    let r = cred.response;
    return {
        id: cred.id,
        rawId: bufToB64url( cred.rawId),
        type: cred.type,
        response: {
            attestationObject: bufToB64url( r.attestationObject),
            clientDataJSON: bufToB64url( r.clientDataJSON),
            transports: r.getTransports ? r.getTransports() : undefined
        },
        extensions: cred.getClientExtensionResults ? cred.getClientExtensionResults() : {}
    };
}

function assertionToJson (cred) {
    let r = cred.response;
    return {
        id: cred.id,
        rawId: bufToB64url( cred.rawId),
        type: cred.type,
        response: {
            authenticatorData: bufToB64url( r.authenticatorData),
            clientDataJSON: bufToB64url( r.clientDataJSON),
            signature: bufToB64url( r.signature),
            userHandle: r.userHandle ? bufToB64url( r.userHandle) : null
        },
        extensions: cred.getClientExtensionResults ? cred.getClientExtensionResults() : {}
    };
}
