const TOKEN_STORAGE_KEY = "openclawBearerToken";

function currentToken() {
  return localStorage.getItem(TOKEN_STORAGE_KEY) ?? "";
}

function saveToken(token) {
  localStorage.setItem(TOKEN_STORAGE_KEY, token.trim());
}

function setStatus(message, isError = false) {
  const status = document.querySelector("#status");
  status.textContent = message;
  status.style.color = isError ? "#b42318" : "#146c43";
}

async function getJson(path, options = {}) {
  const headers = { "Content-Type": "application/json" };
  const token = currentToken();
  if (token.length > 0) {
    headers.Authorization = `Bearer ${token}`;
  }

  const response = await fetch(path, {
    headers,
    ...options
  });

  let body = null;
  const contentType = response.headers.get("content-type") ?? "";
  if (contentType.includes("application/json")) {
    body = await response.json();
  } else {
    const raw = await response.text();
    body = { error: { message: raw || "request failed" } };
  }
  if (!response.ok) {
    const message = body?.error?.message ?? body?.error ?? "request failed";
    const code = body?.error?.code;
    const requestId = body?.requestId;
    const suffix = [code, requestId].filter(Boolean).join(" / ");
    const fullMessage = suffix.length > 0 ? `${message} (${suffix})` : message;
    throw new Error(fullMessage);
  }
  return body;
}

async function postJob(type, payload) {
  return getJson(`/jobs/${type}`, {
    method: "POST",
    body: JSON.stringify(payload)
  });
}

async function loadTemplates() {
  const data = await getJson("/templates");
  const select = document.querySelector("#template-select");
  select.innerHTML = "";

  for (const pack of data.templatePacks) {
    const option = document.createElement("option");
    option.value = pack.id;
    option.textContent = `${pack.name} (${pack.mode})`;
    select.append(option);
  }
}

async function renderInstances() {
  const list = document.querySelector("#instance-list");
  list.innerHTML = "";

  let data;
  try {
    data = await getJson("/instances");
  } catch (error) {
    const li = document.createElement("li");
    li.textContent = `Unable to load instances: ${error.message}`;
    list.append(li);
    return;
  }

  for (const instance of data.instances) {
    const li = document.createElement("li");
    const title = document.createElement("strong");
    title.textContent = instance.name;
    li.append(title);

    const variant = instance.clawVariant ?? "openclaw";
    const publicUrl = instance.uiAccess?.publicUrl ?? "pending";
    const details = document.createElement("span");
    details.textContent = ` [${instance.status}] - ${variant} - ${instance.templatePackId} - UI: ${publicUrl}`;
    li.append(details);

    const actions = document.createElement("span");
    actions.style.marginLeft = "8px";

    for (const [label, job] of [
      ["start", "start-instance"],
      ["stop", "stop-instance"],
      ["delete", "delete-instance"]
    ]) {
      const button = document.createElement("button");
      button.type = "button";
      button.textContent = label;
      button.style.marginLeft = "4px";
      if (label === "start" && instance.status !== "stopped") {
        button.disabled = true;
      }
      if (label === "stop" && instance.status !== "running") {
        button.disabled = true;
      }
      if (label === "delete" && instance.status === "deleted") {
        button.disabled = true;
      }
      button.addEventListener("click", async () => {
        if (label === "delete") {
          const confirmed = window.confirm(
            `Delete instance ${instance.name}? This action cannot be undone.`
          );
          if (!confirmed) {
            return;
          }
        }

        try {
          button.disabled = true;
          await postJob(job, { instance_id: instance.id });
          await renderInstances();
          setStatus(`Instance action succeeded: ${label}.`);
        } catch (error) {
          setStatus(`Action failed (${label}): ${error.message}`, true);
        } finally {
          button.disabled = false;
        }
      });
      actions.append(button);
    }

    li.append(actions);
    list.append(li);
  }
}

document.querySelector("#auth-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const formData = new FormData(event.currentTarget);
  saveToken(`${formData.get("bearerToken") ?? ""}`);
  try {
    await renderInstances();
    setStatus("Bearer token saved.");
  } catch (error) {
    setStatus(`Failed to refresh instances: ${error.message}`, true);
  }
});

document.querySelector("#token-session-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const formData = new FormData(event.currentTarget);
  try {
    const session = await getJson("/auth/session/token", {
      method: "POST",
      body: JSON.stringify({
        instanceId: `${formData.get("instanceId") ?? ""}`,
        accessToken: `${formData.get("accessToken") ?? ""}`
      })
    });
    saveToken(session.token ?? "");
    document.querySelector("#bearer-token").value = currentToken();
    setStatus("Session token created from access token.");
    await renderInstances();
  } catch (error) {
    setStatus(`Access token login failed: ${error.message}`, true);
  }
});

document.querySelector("#wallet-challenge-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const formData = new FormData(event.currentTarget);
  try {
    const challenge = await getJson("/auth/challenge", {
      method: "POST",
      body: JSON.stringify({
        instanceId: `${formData.get("instanceId") ?? ""}`,
        walletAddress: `${formData.get("walletAddress") ?? ""}`
      })
    });
    document.querySelector("#challenge-id").value = challenge.challengeId ?? "";
    document.querySelector("#challenge-message").value = challenge.message ?? "";
    setStatus("Wallet challenge created. Sign the returned challenge message and submit signature.");
  } catch (error) {
    setStatus(`Challenge creation failed: ${error.message}`, true);
  }
});

document.querySelector("#wallet-verify-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const formData = new FormData(event.currentTarget);
  try {
    const session = await getJson("/auth/session/wallet", {
      method: "POST",
      body: JSON.stringify({
        challengeId: `${formData.get("challengeId") ?? ""}`,
        signature: `${formData.get("signature") ?? ""}`
      })
    });
    saveToken(session.token ?? "");
    document.querySelector("#bearer-token").value = currentToken();
    setStatus("Wallet session verified and saved.");
    await renderInstances();
  } catch (error) {
    setStatus(`Wallet verification failed: ${error.message}`, true);
  }
});

document.querySelector("#launch-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const formData = new FormData(event.currentTarget);
  const config = {
    claw_variant: formData.get("clawVariant"),
    ui: {
      expose_public_url: true,
      auth_mode: formData.get("uiAuthMode")
    }
  };
  const subdomain = `${formData.get("uiSubdomain") ?? ""}`.trim();
  if (subdomain.length > 0) {
    config.ui.subdomain = subdomain;
  }

  try {
    await postJob("create-instance", {
      name: formData.get("name"),
      template_pack_id: formData.get("templatePackId"),
      config_json: JSON.stringify(config)
    });
    event.currentTarget.reset();
    await loadTemplates();
    await renderInstances();
    setStatus("Instance launched.");
  } catch (error) {
    setStatus(`Launch failed: ${error.message}`, true);
  }
});

document.querySelector("#bearer-token").value = currentToken();
try {
  await loadTemplates();
  await renderInstances();
} catch (error) {
  setStatus(`Initial load failed: ${error.message}`, true);
}
