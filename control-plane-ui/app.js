const TOKEN_STORAGE_KEY = "openclawBearerToken";

function currentToken() {
  return localStorage.getItem(TOKEN_STORAGE_KEY) ?? "";
}

function saveToken(token) {
  localStorage.setItem(TOKEN_STORAGE_KEY, token.trim());
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

  const body = await response.json();
  if (!response.ok) {
    const message = body?.error ?? "request failed";
    throw new Error(message);
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
    const variant = instance.clawVariant ?? "openclaw";
    const publicUrl = instance.uiAccess?.publicUrl ?? "pending";
    li.innerHTML = `<strong>${instance.name}</strong> [${instance.status}] - ${variant} - ${instance.templatePackId} - UI: ${publicUrl}`;

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
      button.addEventListener("click", async () => {
        await postJob(job, { instanceId: instance.id });
        await renderInstances();
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
  await renderInstances();
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

  await postJob("create-instance", {
    name: formData.get("name"),
    templatePackId: formData.get("templatePackId"),
    configJson: JSON.stringify(config)
  });

  event.currentTarget.reset();
  await loadTemplates();
  await renderInstances();
});

document.querySelector("#bearer-token").value = currentToken();
await loadTemplates();
await renderInstances();
