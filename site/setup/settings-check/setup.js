// The marketplace settings check's App (ADR-0096): posts a fixed manifest to GitHub for the
// hyaloslabs organization, then shows GitHub's one-time code and the command that turns it into the
// App's key and client ID in the maintainer's own terminal. It keeps nothing and sends nothing
// anywhere but GitHub.
"use strict";

const ORG = "hyaloslabs";
const REPO = "hyaloslabs/marketplace";
const NAME = /^[A-Za-z0-9][A-Za-z0-9 ._-]{0,33}$/;
const PERMISSIONS = '{"actions":"read","administration":"read","members":"read","metadata":"read"}';

// One subshell, so a pasted command is read whole before `read` waits for the code, and the code
// and GitHub's answer go when it ends. The code is read hidden, so it is not in the shell's history.
// Nothing is stored unless the App is owned by hyaloslabs and has exactly the manifest's permissions
// and no events.
const COMMAND = [
  "( read -rsp 'Code: ' code; echo",
  "  answer=$(gh api -X POST \"app-manifests/$code/conversions\") &&",
  "  [ \"$(jq -r .owner.login <<<\"$answer\")\" = '" + ORG + "' ] &&",
  "  [ \"$(jq -cS .permissions <<<\"$answer\")\" = '" + PERMISSIONS + "' ] &&",
  "  [ \"$(jq -c .events <<<\"$answer\")\" = '[]' ] &&",
  "  jq -r .pem <<<\"$answer\" | gh secret set SETTINGS_APP_KEY --env settings-check --repo " + REPO + " &&",
  "  gh variable set SETTINGS_APP_CLIENT_ID --env settings-check --repo " + REPO + " --body \"$(jq -r .client_id <<<\"$answer\")\" &&",
  "  echo \"Stored. Install $(jq -r .html_url <<<\"$answer\") on " + REPO + " alone.\" ||",
  "  echo 'Not stored: the App is not what this page asked for, or a step failed. Delete the App.' )",
].join("\n");

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

function copy(button, text) {
  navigator.clipboard.writeText(text).then(
    () => { button.textContent = "Copied"; },
    () => { button.textContent = "Select it and copy"; },
  );
}

async function start() {
  const code = new URLSearchParams(window.location.search).get("code");
  if (code) {
    // Out of the address bar and the history, now that it is on the page.
    window.history.replaceState(null, "", window.location.pathname);
    document.getElementById("code").textContent = code;
    document.getElementById("command").textContent = COMMAND;
    const button = document.getElementById("copy");
    button.addEventListener("click", () => copy(button, code));
    show("done");
    return;
  }
  show("start");
  document.getElementById("create").addEventListener("submit", async (event) => {
    event.preventDefault();
    let manifest;
    try {
      const response = await fetch("manifest.json", { cache: "no-store" });
      manifest = await response.json();
    } catch (e) {
      problem("The page could not read its manifest. Reload it and try again.");
      return;
    }
    const name = document.getElementById("name").value.trim();
    if (name && !NAME.test(name)) {
      problem("An App name is letters, digits, spaces, '.', '_' and '-', up to 34.");
      return;
    }
    manifest.name = name || manifest.name;
    const form = document.createElement("form");
    form.method = "post";
    form.action = "https://github.com/organizations/" + ORG + "/settings/apps/new";
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
