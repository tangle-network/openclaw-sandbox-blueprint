async function getJson(path, options = {}) {
  const response = await fetch(path, {
    headers: { "Content-Type": "application/json" },
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
  const data = await getJson("/instances");
  const list = document.querySelector("#instance-list");
  list.innerHTML = "";

  for (const instance of data.instances) {
    const li = document.createElement("li");
    li.innerHTML = `<strong>${instance.name}</strong> [${instance.status}] - ${instance.templatePackId}`;

    const actions = document.createElement("span");
    actions.style.marginLeft = "8px";

    for (const [label, job] of [
      ["start", "start-hosted-instance"],
      ["stop", "stop-hosted-instance"],
      ["delete", "delete-hosted-instance"]
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

document.querySelector("#launch-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const formData = new FormData(event.currentTarget);
  await postJob("create-hosted-instance", {
    name: formData.get("name"),
    templatePackId: formData.get("templatePackId")
  });

  event.currentTarget.reset();
  await loadTemplates();
  await renderInstances();
});

await loadTemplates();
await renderInstances();
