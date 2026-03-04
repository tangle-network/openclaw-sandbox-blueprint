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
    const runtime = instance.runtime ?? {};
    const setupStatus = runtime.setupStatus ?? "n/a";
    const localUi = runtime.uiLocalUrl ?? "n/a";
    const secured = runtime.hasUiBearerToken ? "secured" : "unsecured";
    const details = document.createElement("span");
    details.textContent = ` [${instance.status}] - ${variant} - ${instance.templatePackId} - UI: ${publicUrl} - local: ${localUi} - setup: ${setupStatus} - ${secured}`;
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

    const setupButton = document.createElement("button");
    setupButton.type = "button";
    setupButton.textContent = "start-setup";
    setupButton.style.marginLeft = "4px";
    if (instance.status !== "running") {
      setupButton.disabled = true;
      setupButton.title = "Start the instance before running setup.";
    }
    setupButton.addEventListener("click", async () => {
      const raw = window.prompt(
        "Optional setup env vars (KEY=VALUE, one per line). Leave empty for none.",
        ""
      );
      const env = {};
      if (raw && raw.trim().length > 0) {
        for (const line of raw.split("\n")) {
          const trimmed = line.trim();
          if (!trimmed) {
            continue;
          }
          const idx = trimmed.indexOf("=");
          if (idx <= 0) {
            setStatus(`Invalid setup env line: ${trimmed}`, true);
            return;
          }
          const key = trimmed.slice(0, idx).trim();
          const value = trimmed.slice(idx + 1);
          env[key] = value;
        }
      }

      try {
        setupButton.disabled = true;
        await getJson(`/instances/${instance.id}/setup/start`, {
          method: "POST",
          body: JSON.stringify({ env })
        });
        await renderInstances();
        setStatus("Setup bootstrap started.");
      } catch (error) {
        setStatus(`Setup start failed: ${error.message}`, true);
      } finally {
        setupButton.disabled = false;
      }
    });
    actions.append(setupButton);

    const accessButton = document.createElement("button");
    accessButton.type = "button";
    accessButton.textContent = "show-access";
    accessButton.style.marginLeft = "4px";
    accessButton.addEventListener("click", async () => {
      try {
        accessButton.disabled = true;
        const access = await getJson(`/instances/${instance.id}/access`);
        const lines = [
          `Instance: ${access.instanceId}`,
          `Auth: ${access.authScheme}`,
          `Bearer: ${access.bearerToken}`,
          `Local UI: ${access.uiLocalUrl ?? "n/a"}`,
          `Public URL: ${access.publicUrl ?? "n/a"}`,
          "",
          "Use header:",
          `Authorization: Bearer ${access.bearerToken}`
        ];
        window.alert(lines.join("\n"));
        setStatus("Fetched instance access credentials.");
      } catch (error) {
        setStatus(`Access lookup failed: ${error.message}`, true);
      } finally {
        accessButton.disabled = false;
      }
    });
    actions.append(accessButton);

    const terminalButton = document.createElement("button");
    terminalButton.type = "button";
    terminalButton.textContent = "terminal-cmd";
    terminalButton.style.marginLeft = "4px";
    terminalButton.disabled = instance.status !== "running";
    terminalButton.addEventListener("click", async () => {
      const command = window.prompt("Terminal command to run", "echo hello");
      if (!command || !command.trim()) {
        return;
      }
      try {
        terminalButton.disabled = true;
        const created = await getJson(`/instances/${instance.id}/terminals`, { method: "POST" });
        const terminalId = created?.data?.sessionId;
        if (!terminalId) {
          throw new Error("terminal session id missing");
        }
        const output = await getJson(`/instances/${instance.id}/terminals/${terminalId}/execute`, {
          method: "POST",
          body: JSON.stringify({ command })
        });
        await getJson(`/instances/${instance.id}/terminals/${terminalId}`, { method: "DELETE" });
        const lines = [
          `Exit: ${output.exitCode ?? "n/a"}`,
          "STDOUT:",
          output.stdout ?? "",
          "",
          "STDERR:",
          output.stderr ?? ""
        ];
        window.alert(lines.join("\n"));
        setStatus("Terminal command executed.");
      } catch (error) {
        setStatus(`Terminal command failed: ${error.message}`, true);
      } finally {
        terminalButton.disabled = false;
      }
    });
    actions.append(terminalButton);

    const sshButton = document.createElement("button");
    sshButton.type = "button";
    sshButton.textContent = "ssh-key";
    sshButton.style.marginLeft = "4px";
    sshButton.disabled = instance.status !== "running";
    sshButton.addEventListener("click", async () => {
      const mode = window.prompt("SSH mode: add or revoke", "add");
      if (!mode) return;
      const username = window.prompt("SSH username", "agent");
      if (!username) return;
      const publicKey = window.prompt("SSH public key (single line)", "");
      if (!publicKey) return;
      const method = mode.toLowerCase().startsWith("r") ? "DELETE" : "POST";
      try {
        sshButton.disabled = true;
        await getJson(`/instances/${instance.id}/ssh`, {
          method,
          body: JSON.stringify({ username, publicKey })
        });
        setStatus(`SSH key ${method === "POST" ? "added" : "revoked"}.`);
      } catch (error) {
        setStatus(`SSH key update failed: ${error.message}`, true);
      } finally {
        sshButton.disabled = false;
      }
    });
    actions.append(sshButton);

    const chatButton = document.createElement("button");
    chatButton.type = "button";
    chatButton.textContent = "chat-once";
    chatButton.style.marginLeft = "4px";
    chatButton.disabled = instance.status !== "running";
    chatButton.addEventListener("click", async () => {
      const prompt = window.prompt("Chat prompt", "hello");
      if (!prompt || !prompt.trim()) return;
      try {
        chatButton.disabled = true;
        const created = await getJson(`/instances/${instance.id}/session/sessions`, {
          method: "POST",
          body: JSON.stringify({ title: "Control UI Session" })
        });
        const sessionId = created?.id;
        if (!sessionId) throw new Error("chat session id missing");
        await getJson(`/instances/${instance.id}/session/sessions/${sessionId}/messages`, {
          method: "POST",
          body: JSON.stringify({ parts: [{ type: "text", text: prompt }] })
        });
        const messages = await getJson(`/instances/${instance.id}/session/sessions/${sessionId}/messages?limit=10`);
        const array = Array.isArray(messages) ? messages : [];
        const assistant = [...array]
          .reverse()
          .find((item) => item?.info?.role === "assistant");
        const reply = assistant?.parts?.[0]?.text ?? "No assistant response yet.";
        window.alert(`Assistant:\n${reply}`);
        setStatus("Chat request completed.");
      } catch (error) {
        setStatus(`Chat request failed: ${error.message}`, true);
      } finally {
        chatButton.disabled = false;
      }
    });
    actions.append(chatButton);

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
