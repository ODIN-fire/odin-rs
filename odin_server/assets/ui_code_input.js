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

// we don't have to import code_input.js here since it is a script
import * as ui from "../odin_server/ui.js";

export function CodeInput(eid, template, lang, width, height) {
    let e = document.createElement("code-input");
    e.classList.add("ui_code_input");

    if (eid) e.id = eid;
    // needs to use setAttribute as those are intercepted
    e.setAttribute("template", template);
    e.setAttribute("language", lang);
    e.spellcheck = false;

    if (width) {
        e.style.minWidth = width;
        e.style.maxWidth = width;
    }
    if (height) {
        e.style.minHeight = height;
        e.style.maxHeight = height;
    }

    // make sure we scroll to keep the cursor visible
    e.addEventListener("input", () => {
        e.scrollTop = e.scrollHeight;
    });

    return e;
}

function initializeCodeInput(e) {
    if (e.children.length == 0) {
        let te = document.createElement("textarea");
        te.classList.add("ui_textarea");
        te.dataset.codeInputFallback = true;
        e.appendChild(te);
    }
}
ui.registerUiClassInitializer("ui_code_input", initializeCodeInput);

export function setCode(o, code) {
    let e = getCodeInput(o);
    if (e) {
        if (e.textareaElement) {
            e.textareaElement.value = code ? code : "";
            e.update();
        }
    }
}

export function clearCode(o) {
    setCode(o, null);
}

export function getCode(o) {
    let e = getCodeInput(o);
    if (e) {
        if (e.textareaElement) {
            return e.textareaElement.value;
        }
    }
    return null;
}

function getCodeInput(o) {
    let e = ui.elementOf(o);
    if (e && e.classList.contains("ui_code_input")) {
        return e;
    }
    throw "not a code-input";
}

//--- standard highlighters

function escapeHtml(str) {
  return str
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

//--- Fortran NameList parser

// One master regex, alternatives ordered so quotes/comments are
// matched whole before anything inside them can be mistaken for code.
const NML_TOKEN_RE = /(?<comment>![^\n]*)|(?<string>'(?:[^']|'')*'|"(?:[^"]|"")*")|(?<nlend>&END\b)|(?<nlstart>&[A-Za-z_]\w*)|(?<slash>\/)|(?<boolean>\.(?:TRUE|FALSE|T|F)\.)|(?<number>[+-]?(?:\d+\.\d*|\.\d+|\d+)(?:[dDeE][+-]?\d+)?)|(?<ident>[A-Za-z_]\w*)|(?<op>[=,():*])/gi;

function highlightNamelist(resultElement) {
    // resultElement is the <code> inside <pre>. code-input has already set
    // its innerHTML to the escaped raw text before calling this function;
    // textContent gives us the raw (decoded) source back regardless.
    const source = resultElement.textContent;

    let html = "";
    let lastIndex = 0;
    let match;

    NML_TOKEN_RE.lastIndex = 0;
    while ((match = NML_TOKEN_RE.exec(source)) !== null) {
        if (match.index > lastIndex) {
            html += escapeHtml(source.slice(lastIndex, match.index));
        }

        const g = match.groups;
        const text = escapeHtml(match[0]);

        if (g.comment) html += `<span class="token-comment">${text}</span>`;
        else if (g.string) html += `<span class="token-string">${text}</span>`;
        else if (g.nlend)
            html += `<span class="token-namelist-delim">${text}</span>`;
        else if (g.nlstart)
            html += `<span class="token-namelist-delim">${text}</span>`;
        else if (g.slash)
            html += `<span class="token-namelist-delim">${text}</span>`;
        else if (g.boolean)
            html += `<span class="token-boolean">${text}</span>`;
        else if (g.number) html += `<span class="token-number">${text}</span>`;
        else if (g.ident) html += `<span class="token-variable">${text}</span>`;
        else if (g.op) html += `<span class="token-operator">${text}</span>`;

        lastIndex = NML_TOKEN_RE.lastIndex;
    }
    html += escapeHtml(source.slice(lastIndex));

    resultElement.innerHTML = html;
}

function instantiateNmlIndent() {
    // (defaultSpaces, numSpaces, bracketPairs, escTabToChangeFocus)
    // 4-space indentation; { } and [ ] auto-indent/unindent, which
    // covers JSON's only two bracket types.
    return new codeInput.plugins.Indent(true, 4, true);
}

codeInput.registerTemplate(
    "nml-highlight",
    new codeInput.Template(highlightNamelist, true, true, false, [ instantiateNmlIndent() ])
);

// [ new codeInput.plugins.Indent(true, 2) ]

//--- JSON parsing

const JSON_TOKEN_RE = /(?<string>"(?:\\.|[^"\\])*")|(?<number>-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|(?<keyword>true|false|null)|(?<punct>[{}\[\],:])/g;

function highlightJson(resultElement) {
  // resultElement is the <code> inside <pre>. code-input has already set
  // its innerHTML to the escaped raw text before calling this function;
  // textContent gives us the raw (decoded) source back regardless.
  const source = resultElement.textContent;

  let html = "";
  let lastIndex = 0;
  let match;

  JSON_TOKEN_RE.lastIndex = 0;
  while ((match = JSON_TOKEN_RE.exec(source)) !== null) {
    if (match.index > lastIndex) {
      html += escapeHtml(source.slice(lastIndex, match.index));
    }

    const g = match.groups;
    const text = escapeHtml(match[0]);

    if (g.string) {
      // A string is a "key" if, skipping whitespace, the next
      // character after it is a colon.
      let i = JSON_TOKEN_RE.lastIndex;
      while (i < source.length && /\s/.test(source[i])) i++;
      const isKey = source[i] === ":";
      html += `<span class="${isKey ? "token-key" : "token-string"}">${text}</span>`;
    } else if (g.number) {
      html += `<span class="token-number">${text}</span>`;
    } else if (g.keyword) {
      const cls = match[0] === "null" ? "token-const" : "token-boolean";
      html += `<span class="${cls}">${text}</span>`;
    } else if (g.punct) {
      html += `<span class="token-punctuation">${text}</span>`;
    }

    lastIndex = JSON_TOKEN_RE.lastIndex;
  }
  html += escapeHtml(source.slice(lastIndex));

  resultElement.innerHTML = html;
}

function instantiateJsonIndent() {
    // (defaultSpaces, numSpaces, bracketPairs, escTabToChangeFocus)
    // 4-space indentation; { } and [ ] auto-indent/unindent, which
    // covers JSON's only two bracket types.
    return new codeInput.plugins.Indent(true, 4, { "{": "}", "[": "]" }, true);
}

codeInput.registerTemplate(
  "json-highlight",
  new codeInput.Template( highlightJson, true, true, false, [ instantiateJsonIndent() ])
);


//--- WKT parsing

const WKT_TOKEN_RE = /(?<srid>SRID\s*=\s*\d+\s*;)|(?<geomtype>\b(?:MULTIPOLYGON|MULTILINESTRING|MULTIPOINT|GEOMETRYCOLLECTION|POLYHEDRALSURFACE|COMPOUNDCURVE|CURVEPOLYGON|CIRCULARSTRING|MULTICURVE|MULTISURFACE|LINESTRING|POLYGON|TRIANGLE|POINT|TIN)\b)|(?<dim>\b(?:ZM|Z|M)\b)|(?<empty>\bEMPTY\b)|(?<number>-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|(?<punct>[(),])/gi;

function highlightWkt(resultElement) {
  // resultElement is the <code> inside <pre>. code-input has already set
  // its innerHTML to the escaped raw text before calling this function;
  // textContent gives us the raw (decoded) source back regardless.
  const source = resultElement.textContent;

  let html = "";
  let lastIndex = 0;
  let match;

  WKT_TOKEN_RE.lastIndex = 0;
  while ((match = WKT_TOKEN_RE.exec(source)) !== null) {
    if (match.index > lastIndex) {
      html += escapeHtml(source.slice(lastIndex, match.index));
    }

    const g = match.groups;
    const text = escapeHtml(match[0]);

    if (g.srid)          html += `<span class="token-key">${text}</span>`;
    else if (g.geomtype) html += `<span class="token-key">${text}</span>`;
    else if (g.dim)      html += `<span class="token-operator">${text}</span>`;
    else if (g.empty)    html += `<span class="token-key">${text}</span>`;
    else if (g.number)   html += `<span class="token-number">${text}</span>`;
    else if (g.punct)    html += `<span class="token-punctuation">${text}</span>`;

    lastIndex = WKT_TOKEN_RE.lastIndex;
  }
  html += escapeHtml(source.slice(lastIndex));

  resultElement.innerHTML = html;
}

codeInput.registerTemplate(
  "wkt-highlight",
  new codeInput.Template(highlightWkt, true, true, false, [])
);
