(() => {
  const STORAGE_KEY = "aion2-flight-log-v1";
  const RESET_HOUR_UTC = 7; // Global daily / weekly reset

  const lists = {
    daily: document.getElementById("daily-list"),
    weekly: document.getElementById("weekly-list"),
    shops: document.getElementById("shops-list"),
  };

  const dailyCountdown = document.getElementById("daily-countdown");
  const weeklyCountdown = document.getElementById("weekly-countdown");
  const notesField = document.getElementById("notes-field");

  /** @returns {Date} */
  function periodAnchor(date = new Date()) {
    const d = new Date(date);
    if (d.getUTCHours() < RESET_HOUR_UTC) {
      d.setUTCDate(d.getUTCDate() - 1);
    }
    d.setUTCHours(RESET_HOUR_UTC, 0, 0, 0);
    return d;
  }

  function dailyPeriodKey(date = new Date()) {
    return periodAnchor(date).toISOString().slice(0, 10);
  }

  function weeklyPeriodKey(date = new Date()) {
    const anchor = periodAnchor(date);
    const daysSinceWed = (anchor.getUTCDay() + 4) % 7;
    anchor.setUTCDate(anchor.getUTCDate() - daysSinceWed);
    return anchor.toISOString().slice(0, 10);
  }

  function nextDailyReset(date = new Date()) {
    const next = new Date(date);
    next.setUTCHours(RESET_HOUR_UTC, 0, 0, 0);
    if (date >= next) next.setUTCDate(next.getUTCDate() + 1);
    return next;
  }

  function nextWeeklyReset(date = new Date()) {
    const next = nextDailyReset(date);
    while (next.getUTCDay() !== 3) {
      next.setUTCDate(next.getUTCDate() + 1);
    }
    return next;
  }

  function formatCountdown(target, now = new Date()) {
    let ms = Math.max(0, target - now);
    const days = Math.floor(ms / 86400000);
    ms %= 86400000;
    const hours = Math.floor(ms / 3600000);
    ms %= 3600000;
    const minutes = Math.floor(ms / 60000);
    if (days > 0) return `${days}d ${hours}h`;
    if (hours > 0) return `${hours}h ${minutes}m`;
    const seconds = Math.floor((ms % 60000) / 1000);
    return `${minutes}m ${seconds}s`;
  }

  function defaultState() {
    const defs = window.AION2_DEFAULTS;
    const section = (items) => ({
      items: items.map((item) => ({ ...item, custom: false })),
      checked: {},
    });

    return {
      dailyKey: dailyPeriodKey(),
      weeklyKey: weeklyPeriodKey(),
      daily: section(defs.daily),
      weekly: section(defs.weekly),
      shops: section(defs.shops),
      notes: "",
    };
  }

  function loadState() {
    let state;
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      state = raw ? JSON.parse(raw) : defaultState();
    } catch {
      state = defaultState();
    }

    // Ensure shape for older or partial saves
    const base = defaultState();
    state = {
      ...base,
      ...state,
      daily: { ...base.daily, ...(state.daily || {}) },
      weekly: { ...base.weekly, ...(state.weekly || {}) },
      shops: { ...base.shops, ...(state.shops || {}) },
    };

    applyPeriodResets(state);
    return state;
  }

  function applyPeriodResets(state) {
    const dailyKey = dailyPeriodKey();
    const weeklyKey = weeklyPeriodKey();

    if (state.dailyKey !== dailyKey) {
      state.daily.checked = {};
      state.dailyKey = dailyKey;
    }

    if (state.weeklyKey !== weeklyKey) {
      state.weekly.checked = {};
      state.shops.checked = {};
      state.weeklyKey = weeklyKey;
    }
  }

  function saveState() {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  }

  let state = loadState();

  function renderSection(name) {
    const ul = lists[name];
    const section = state[name];
    ul.replaceChildren();

    section.items.forEach((item) => {
      const li = document.createElement("li");
      const checked = Boolean(section.checked[item.id]);
      if (checked) li.classList.add("is-done");

      const input = document.createElement("input");
      input.type = "checkbox";
      input.className = "task-check";
      input.id = `${name}-${item.id}`;
      input.checked = checked;
      input.addEventListener("change", () => {
        if (input.checked) section.checked[item.id] = true;
        else delete section.checked[item.id];
        li.classList.toggle("is-done", input.checked);
        updateProgress(name);
        saveState();
      });

      const body = document.createElement("div");
      body.className = "task-body";

      const label = document.createElement("label");
      label.className = "task-label";
      label.htmlFor = input.id;
      label.textContent = item.label;

      const meta = document.createElement("div");
      meta.className = "task-meta";

      if (item.tag) {
        const tag = document.createElement("span");
        tag.className = "tag";
        tag.textContent = item.tag;
        meta.append(tag);
      }

      if (item.priority) {
        const pri = document.createElement("span");
        pri.className = `tag priority-${item.priority}`;
        pri.textContent = item.priority;
        meta.append(pri);
      }

      body.append(label, meta);

      if (item.note) {
        const note = document.createElement("p");
        note.className = "task-note";
        note.textContent = item.note;
        body.append(note);
      }

      li.append(input, body);

      if (item.custom) {
        const remove = document.createElement("button");
        remove.type = "button";
        remove.className = "task-remove";
        remove.setAttribute("aria-label", `Remove ${item.label}`);
        remove.textContent = "✕";
        remove.addEventListener("click", () => {
          section.items = section.items.filter((x) => x.id !== item.id);
          delete section.checked[item.id];
          renderSection(name);
          updateProgress(name);
          saveState();
        });
        li.append(remove);
      } else {
        const spacer = document.createElement("span");
        spacer.setAttribute("aria-hidden", "true");
        li.append(spacer);
      }

      ul.append(li);
    });

    updateProgress(name);
  }

  function updateProgress(name) {
    const section = state[name];
    const total = section.items.length;
    const done = section.items.filter((item) => section.checked[item.id]).length;
    const wrap = document.querySelector(`[data-progress-for="${name}"]`);
    if (!wrap) return;
    wrap.querySelector(".progress-text").textContent = `${done} / ${total}`;
    const pct = total === 0 ? 0 : Math.round((done / total) * 100);
    wrap.querySelector(".progress-fill").style.width = `${pct}%`;
  }

  function renderAll() {
    renderSection("daily");
    renderSection("weekly");
    renderSection("shops");
    notesField.value = state.notes || "";
  }

  function updateCountdowns() {
    const now = new Date();
    dailyCountdown.textContent = formatCountdown(nextDailyReset(now), now);
    weeklyCountdown.textContent = formatCountdown(nextWeeklyReset(now), now);
  }

  function tick() {
    const beforeDaily = state.dailyKey;
    const beforeWeekly = state.weeklyKey;
    applyPeriodResets(state);

    if (state.dailyKey !== beforeDaily || state.weeklyKey !== beforeWeekly) {
      saveState();
      renderAll();
    }

    updateCountdowns();
  }

  document.querySelectorAll(".add-form").forEach((form) => {
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      const name = form.dataset.add;
      const input = form.elements.label;
      const label = String(input.value || "").trim();
      if (!label) return;

      const id = `custom-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
      state[name].items.push({
        id,
        label,
        custom: true,
        tag: "custom",
        priority: "medium",
      });
      input.value = "";
      renderSection(name);
      saveState();
    });
  });

  notesField.addEventListener("input", () => {
    state.notes = notesField.value;
    saveState();
  });

  document.getElementById("reset-all").addEventListener("click", () => {
    if (!confirm("Clear all checked items for the current periods?")) return;
    state.daily.checked = {};
    state.weekly.checked = {};
    state.shops.checked = {};
    saveState();
    renderAll();
  });

  renderAll();
  tick();
  setInterval(tick, 1000);
})();
