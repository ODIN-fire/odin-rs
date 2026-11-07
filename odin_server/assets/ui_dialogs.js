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

// themed replacements for the native browser alert(), confirm() and prompt() dialogs plus portable file open/save
// functions. Dialogs are HTML <dialog> elements shown with showModal(), i.e. they are centered, modal and in the
// top layer (above all windows, regardless of z-index). All functions are async - callers have to await the result:
//
//     if (await dialogs.confirm("remove entry?")) { ... }
//
// Note this is only meant for dialogs with simple elements (text, text input, buttons). Tooltips and popup menus
// of ui.js widgets are not rendered on top of modal dialogs.

var openDialogs = [];

export function isModalDialogOpen() {
    return openDialogs.length > 0;
}

/// show a message. Resolves once the dialog is closed
export async function alert (msg, title="Alert") {
    let okButton = createButton("OK", "ok");
    let dlg = createDialog( title, [createMessage(msg)], [okButton]);
    await runDialog( dlg, okButton);
}

/// ask for confirmation. Resolves to true if the user clicked "OK", false otherwise (including Escape)
export async function confirm (msg, title="Confirm", okLabel="OK", cancelLabel="Cancel") {
    let okButton = createButton(okLabel, "ok");
    let dlg = createDialog( title, [createMessage(msg)], [createButton(cancelLabel, "cancel"), okButton]);
    return (await runDialog( dlg, okButton)) === "ok";
}

/// ask for a text input. Resolves to the entered text or null if the dialog was cancelled
export async function prompt (msg, defaultValue="", title="Input") {
    let input = createTextInput(defaultValue);
    let dlg = createDialog( title, [createMessage(msg), input], [createButton("Cancel", "cancel"), createButton("OK", "ok")]);
    input.addEventListener("keydown", (event) => { if (event.key == "Enter") dlg.close("ok"); });

    return (await runDialog( dlg, input)) === "ok" ? input.value : null;
}

/// open the system file picker and resolve to an array of selected File objects (empty if cancelled).
/// `accept` is a comma separated list of extensions and/or mime types (e.g. ".json,image/*").
/// Note this has to be called from a user action (click or keydown handler) or browsers will ignore it
export function openFiles (accept=undefined, multiple=false) {
    return new Promise( (resolve) => {
        let input = document.createElement("INPUT");
        input.type = "file";
        if (accept) input.accept = accept;
        input.multiple = multiple;
        input.style.display = "none";

        let done = (files) => { input.remove(); resolve(files); };
        input.addEventListener("change", () => done( Array.from(input.files)), {once: true});
        input.addEventListener("cancel", () => done( []), {once: true});

        document.body.appendChild(input); // Safari needs it to be in the document
        input.click();
    });
}

/// save `data` as a local file. `data` can be a Blob, string, ArrayBuffer, typed array, a plain object
/// (saved as JSON) or a Promise for any of these (e.g. from canvas.toBlob(), which can be started before calling us).
/// Unless `opts.prompt` is false this first shows a dialog to edit the filename.
/// The file is saved as a download (i.e. location depends on browser settings) unless `opts.native` is set and the
/// browser supports `showSaveFilePicker()` (only Chromium based browsers), in which case the system save dialog is used.
/// Resolves to the name of the saved file or null if the user cancelled
export async function saveFile (data, filename, opts={}) {
    let title = opts.title || "Save File";

    if (opts.native && window.showSaveFilePicker) {
        try {
            let handle = await window.showSaveFilePicker({ suggestedName: filename });
            let writable = await handle.createWritable();
            await writable.write( await toBlob( data, opts.mimeType));
            await writable.close();
            return handle.name;
        } catch (e) {
            if (e.name == "AbortError") return null; // user cancelled
            // SecurityError: no user activation (e.g. after other async ops) - fall back to dialog + download
            if (e.name != "SecurityError") throw e;
        }
    }

    if (opts.prompt !== false) {
        filename = await prompt( "file name:", filename, title);
        if (!filename) return null;
    }

    let blob = await toBlob( data, opts.mimeType);
    let url = URL.createObjectURL(blob);
    let a = document.createElement("A");
    a.href = url;
    a.download = filename;
    a.style.display = "none";
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout( () => URL.revokeObjectURL(url), 10000); // give the browser time to start the download

    return filename;
}

//--- internal functions

async function toBlob (data, mimeType) {
    data = await data;
    if (data instanceof Blob) return data;
    if (typeof data === "string") return new Blob( [data], {type: mimeType || "text/plain"});
    if (data instanceof ArrayBuffer || ArrayBuffer.isView(data)) return new Blob( [data], {type: mimeType || "application/octet-stream"});
    return new Blob( [JSON.stringify(data, null, 2)], {type: mimeType || "application/json"});
}

function createDialog (title, contents, buttons) {
    let dlg = document.createElement("DIALOG");
    dlg.classList.add("ui_dialog");

    let tb = document.createElement("DIV");
    tb.classList.add("ui_titlebar");
    tb.textContent = title;
    dlg.appendChild(tb);

    let content = document.createElement("DIV");
    content.classList.add("ui_dialog_content");
    contents.forEach( e => content.appendChild(e));
    dlg.appendChild(content);

    let buttonBar = document.createElement("DIV");
    buttonBar.classList.add("ui_dialog_buttons");
    buttons.forEach( b => {
        b.addEventListener("click", () => dlg.close(b.dataset.result));
        buttonBar.appendChild(b);
    });
    buttonBar.addEventListener("keydown", (event) => { // Left/Right arrows move focus between buttons, Enter activates
        if (event.key == "ArrowLeft") moveButtonFocus( buttons, -1);
        else if (event.key == "ArrowRight") moveButtonFocus( buttons, 1);
        else if (event.key == "Enter" && buttons.includes( document.activeElement)) {
            event.preventDefault(); // no additional native activation
            document.activeElement.click();
        }
    });
    dlg.appendChild(buttonBar);

    // keep keys from reaching global (document/window) key handlers - the Escape default action (cancel) still works
    dlg.addEventListener("keydown", (event) => event.stopPropagation());

    return dlg;
}

// resolves to the returnValue of the dialog ("" if cancelled with Escape)
function runDialog (dlg, focusElement) {
    return new Promise( (resolve) => {
        dlg.addEventListener("close", () => {
            let idx = openDialogs.indexOf(dlg);
            if (idx >= 0) openDialogs.splice(idx, 1);
            dlg.remove();
            resolve(dlg.returnValue);
        }, {once: true});

        if (focusElement) focusElement.autofocus = true; // picked up by showModal() instead of the first focusable element

        document.body.appendChild(dlg);
        dlg.showModal();
        openDialogs.push(dlg);

        if (focusElement) {
            focusElement.focus(); // in case the browser does not honor autofocus for dynamically created dialogs
            if (focusElement.type == "text") focusElement.select();
        }
    });
}

function moveButtonFocus (buttons, delta) {
    let idx = buttons.indexOf( document.activeElement);
    if (idx >= 0) {
        let n = buttons.length;
        buttons[(idx + delta + n) % n].focus();
    }
}

function createMessage (msg) {
    let e = document.createElement("DIV");
    e.classList.add("ui_dialog_msg");
    e.textContent = msg;
    return e;
}

function createTextInput (value) {
    let e = document.createElement("INPUT");
    e.type = "text";
    e.classList.add("ui_field", "input", "ui_dialog_input");
    e.value = value;
    return e;
}

function createButton (label, result) {
    let e = document.createElement("INPUT");
    e.type = "button";
    e.tabIndex = 0; // make button tab-traversable (not default on Safari)
    e.classList.add("ui_button");
    e.value = label;
    e.dataset.result = result;
    return e;
}
