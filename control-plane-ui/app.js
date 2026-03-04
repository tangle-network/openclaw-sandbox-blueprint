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

function parseByteSequence(raw) {
  const trimmed = `${raw ?? ""}`.trim();
  if (!trimmed) {
    return [];
  }

  if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
    const parsed = JSON.parse(trimmed);
    if (!Array.isArray(parsed)) {
      throw new Error("Expected JSON array for byte sequence.");
    }
    return parsed.map((value) => {
      const num = Number(value);
      if (!Number.isInteger(num) || num < 0 || num > 255) {
        throw new Error(`Invalid byte value: ${value}`);
      }
      return num;
    });
  }

  const noPrefix = trimmed.startsWith("0x") ? trimmed.slice(2) : trimmed;
  if (!/^[0-9a-fA-F]*$/.test(noPrefix) || noPrefix.length % 2 !== 0) {
    throw new Error(
      "Byte sequence must be JSON array or even-length hex string (optionally 0x-prefixed)."
    );
  }
  const out = [];
  for (let i = 0; i < noPrefix.length; i += 2) {
    out.push(Number.parseInt(noPrefix.slice(i, i + 2), 16));
  }
  return out;
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
    const executionTarget = instance.executionTarget ?? "standard";
    details.textContent = ` [${instance.status}] - ${variant} - ${instance.templatePackId} - target: ${executionTarget} - UI: ${publicUrl} - local: ${localUi} - setup: ${setupStatus} - ${secured}`;
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
      button.disabled = true;
      button.title = "Lifecycle job submission is on-chain only and not exposed via operator API.";
      button.addEventListener("click", async () => {
        setStatus(
          `Lifecycle action '${label}' is on-chain only. Use Tangle job submission for ${job}.`,
          true
        );
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

    const teePublicKeyButton = document.createElement("button");
    teePublicKeyButton.type = "button";
    teePublicKeyButton.textContent = "tee-public-key";
    teePublicKeyButton.style.marginLeft = "4px";
    teePublicKeyButton.disabled = executionTarget !== "tee";
    if (executionTarget !== "tee") {
      teePublicKeyButton.title = "Only available for TEE-targeted instances.";
    }
    teePublicKeyButton.addEventListener("click", async () => {
      try {
        teePublicKeyButton.disabled = true;
        const result = await getJson(`/instances/${instance.id}/tee/public-key`);
        const payload = result?.publicKey ?? {};
        const lines = [
          `Instance: ${result?.instanceId ?? instance.id}`,
          `Algorithm: ${payload.algorithm ?? "n/a"}`,
          `Public key bytes: ${JSON.stringify(payload.publicKeyBytes ?? [])}`,
          `Attestation tee_type: ${payload?.attestation?.tee_type ?? "n/a"}`,
          `Attestation measurement bytes: ${JSON.stringify(payload?.attestation?.measurement ?? [])}`
        ];
        window.alert(lines.join("\n"));
        setStatus("Fetched TEE public key.");
      } catch (error) {
        setStatus(`TEE public key failed: ${error.message}`, true);
      } finally {
        teePublicKeyButton.disabled = executionTarget !== "tee";
      }
    });
    actions.append(teePublicKeyButton);

    const teeAttestationButton = document.createElement("button");
    teeAttestationButton.type = "button";
    teeAttestationButton.textContent = "tee-attestation";
    teeAttestationButton.style.marginLeft = "4px";
    teeAttestationButton.disabled = executionTarget !== "tee";
    if (executionTarget !== "tee") {
      teeAttestationButton.title = "Only available for TEE-targeted instances.";
    }
    teeAttestationButton.addEventListener("click", async () => {
      try {
        teeAttestationButton.disabled = true;
        const result = await getJson(`/instances/${instance.id}/tee/attestation`);
        window.alert(JSON.stringify(result, null, 2));
        setStatus("Fetched TEE attestation.");
      } catch (error) {
        setStatus(`TEE attestation failed: ${error.message}`, true);
      } finally {
        teeAttestationButton.disabled = executionTarget !== "tee";
      }
    });
    actions.append(teeAttestationButton);

    const teeSealedButton = document.createElement("button");
    teeSealedButton.type = "button";
    teeSealedButton.textContent = "tee-sealed";
    teeSealedButton.style.marginLeft = "4px";
    teeSealedButton.disabled = executionTarget !== "tee";
    if (executionTarget !== "tee") {
      teeSealedButton.title = "Only available for TEE-targeted instances.";
    }
    teeSealedButton.addEventListener("click", async () => {
      const algorithm = window.prompt(
        "Sealed secret algorithm",
        "x25519-xsalsa20-poly1305"
      );
      if (!algorithm || !algorithm.trim()) {
        return;
      }

      const ciphertextRaw = window.prompt(
        "Ciphertext bytes (JSON array like [1,2,3] or hex like 0x010203)",
        ""
      );
      if (!ciphertextRaw || !ciphertextRaw.trim()) {
        return;
      }

      const nonceRaw = window.prompt(
        "Nonce bytes (JSON array like [1,2,3] or hex like 0x010203)",
        ""
      );
      if (!nonceRaw || !nonceRaw.trim()) {
        return;
      }

      let ciphertext;
      let nonce;
      try {
        ciphertext = parseByteSequence(ciphertextRaw);
        nonce = parseByteSequence(nonceRaw);
      } catch (error) {
        setStatus(`Invalid sealed-secret bytes: ${error.message}`, true);
        return;
      }

      try {
        teeSealedButton.disabled = true;
        const result = await getJson(`/instances/${instance.id}/tee/sealed-secrets`, {
          method: "POST",
          body: JSON.stringify({
            sealedSecret: {
              algorithm: algorithm.trim(),
              ciphertext,
              nonce
            }
          })
        });
        window.alert(JSON.stringify(result, null, 2));
        setStatus("Sent sealed secrets payload to TEE instance.");
      } catch (error) {
        setStatus(`TEE sealed-secrets failed: ${error.message}`, true);
      } finally {
        teeSealedButton.disabled = executionTarget !== "tee";
      }
    });
    actions.append(teeSealedButton);

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
  setStatus(
    "Instance creation is on-chain only. Submit create/start/stop/delete via Tangle jobs, then use this UI for owner-scoped setup, terminal, chat, SSH, and TEE secret/attestation flows.",
    true
  );
});

document.querySelector("#bearer-token").value = currentToken();
try {
  await loadTemplates();
  await renderInstances();
} catch (error) {
  setStatus(`Initial load failed: ${error.message}`, true);
}
