// The Hyalos setup page (ADR-0084): posts a fixed manifest to GitHub, then shows the one-time code
// GitHub sends back. It keeps nothing and sends nothing anywhere but GitHub.
"use strict";

const ACCOUNT = /^[A-Za-z0-9-]{1,39}$/;

function show(id) {
  for (const section of ["start", "done"]) {
    document.getElementById(section).hidden = section !== id;
  }
}

function problem(text) {
  const p = document.getElementById("problem");
  p.textContent = text;
  p.hidden = false;
}

async function start() {
  const code = new URLSearchParams(window.location.search).get("code");
  if (code) {
    // Out of the address bar and the history, now that it is on the page.
    window.history.replaceState(null, "", window.location.pathname);
    document.getElementById("code").textContent = code;
    document.getElementById("copy").addEventListener("click", () => {
      navigator.clipboard.writeText(code).then(
        () => { document.getElementById("copy").textContent = "Copied"; },
        () => { document.getElementById("copy").textContent = "Select it and copy"; },
      );
    });
    show("done");
    return;
  }
  show("start");
  const owner = document.getElementById("owner");
  const name = document.getElementById("name");
  owner.addEventListener("input", () => {
    name.placeholder = ("hyalos-" + owner.value.trim()).slice(0, 34);
  });
  document.getElementById("create").addEventListener("submit", async (event) => {
    event.preventDefault();
    const account = owner.value.trim();
    if (!ACCOUNT.test(account)) {
      problem("A GitHub account is letters, digits and '-'.");
      return;
    }
    let manifest;
    try {
      const response = await fetch("manifest.json", { cache: "no-store" });
      manifest = await response.json();
    } catch (e) {
      problem("The page could not read its manifest. Reload it and try again.");
      return;
    }
    manifest.name = (name.value.trim() || "hyalos-" + account).slice(0, 34);
    const org = document.getElementById("org").checked;
    const form = document.createElement("form");
    form.method = "post";
    form.action = org
      ? "https://github.com/organizations/" + encodeURIComponent(account) + "/settings/apps/new"
      : "https://github.com/settings/apps/new";
    const field = document.createElement("input");
    field.type = "hidden";
    field.name = "manifest";
    field.value = JSON.stringify(manifest);
    form.appendChild(field);
    document.body.appendChild(form);
    form.submit();
  });
}

document.addEventListener("DOMContentLoaded", start);
