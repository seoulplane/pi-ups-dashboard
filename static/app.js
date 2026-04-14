const ids = {
  globalStatus: document.getElementById("global-status"),
  lastUpdated: document.getElementById("last-updated"),
  refreshIn: document.getElementById("refresh-in"),
  warning: document.getElementById("warning"),
  cpu: document.getElementById("cpu"),
  ram: document.getElementById("ram"),
  storage: document.getElementById("storage"),
  temp: document.getElementById("temp"),
  tempTrend: document.getElementById("temp-trend"),
  download: document.getElementById("download"),
  upload: document.getElementById("upload"),
  networkTrend: document.getElementById("network-trend"),
  upsSource: document.getElementById("ups-source"),
  upsStatus: document.getElementById("ups-status"),
  upsBattery: document.getElementById("ups-battery"),
  upsLoad: document.getElementById("ups-load"),
  upsLinev: document.getElementById("ups-linev"),
  upsRuntime: document.getElementById("ups-runtime"),
  upsLastxfer: document.getElementById("ups-lastxfer"),
};

let secondsRemaining = 30;
let lastSuccess = null;
const MAX_HISTORY = 20;

const history = {
  temperature: [],
  download: [],
  upload: [],
};

function pushHistoryPoint(series, value) {
  series.push(value);
  if (series.length > MAX_HISTORY) {
    series.shift();
  }
}

function buildPolylinePoints(values, width, height, min, max) {
  if (values.length === 0) {
    return "";
  }

  const step = values.length > 1 ? width / (values.length - 1) : width;
  return values
    .map((value, index) => {
      const normalized = (value - min) / Math.max(0.0001, max - min);
      const x = index * step;
      const y = height - normalized * height;
      return `${x.toFixed(2)},${y.toFixed(2)}`;
    })
    .join(" ");
}

function drawSingleTrend(svg, values, strokeColor) {
  const width = 320;
  const height = 72;

  if (!values.length) {
    svg.innerHTML = "";
    return;
  }

  const min = Math.min(...values);
  const max = Math.max(...values) + (Math.max(...values) === min ? 1 : 0);
  const points = buildPolylinePoints(values, width, height, min, max);

  svg.innerHTML = `
    <line x1="0" y1="${height}" x2="${width}" y2="${height}" stroke="#cfd9d1" stroke-width="1" />
    <polyline fill="none" stroke="${strokeColor}" stroke-width="2.5" points="${points}" />
  `;
}

function drawDualTrend(svg, firstValues, secondValues) {
  const width = 900;
  const height = 220;
  const all = [...firstValues, ...secondValues];

  if (!all.length) {
    svg.innerHTML = "";
    return;
  }

  const min = Math.min(...all);
  const max = Math.max(...all) + (Math.max(...all) === min ? 1 : 0);
  const downPoints = buildPolylinePoints(firstValues, width, height, min, max);
  const upPoints = buildPolylinePoints(secondValues, width, height, min, max);

  svg.innerHTML = `
    <line x1="0" y1="${height}" x2="${width}" y2="${height}" stroke="#cfd9d1" stroke-width="1" />
    <polyline fill="none" stroke="#0f4c5c" stroke-width="3" points="${downPoints}" />
    <polyline fill="none" stroke="#1f7a4f" stroke-width="2.4" points="${upPoints}" />
  `;
}

function formatRate(bytesPerSecond) {
  const kb = bytesPerSecond / 1024;
  if (kb < 1024) {
    return `${kb.toFixed(0)} KB/s`;
  }
  return `${(kb / 1024).toFixed(2)} MB/s`;
}

function applyStatus(status) {
  ids.globalStatus.textContent = status;
  ids.globalStatus.classList.remove("healthy", "warning", "critical");

  const s = status.toLowerCase();
  if (s.includes("critical") || s.includes("stale")) {
    ids.globalStatus.classList.add("critical");
  } else if (s.includes("warning")) {
    ids.globalStatus.classList.add("warning");
  } else {
    ids.globalStatus.classList.add("healthy");
  }
}

function render(data) {
  applyStatus(data.status || "Healthy");
  ids.lastUpdated.textContent = `Last update: ${new Date(data.updated_at).toLocaleTimeString()}`;

  ids.cpu.textContent = data.system.cpu_percent;
  ids.ram.textContent = data.system.ram_percent;
  ids.storage.textContent = data.system.storage_percent;
  ids.temp.textContent = data.system.temperature_c.toFixed(1);

  ids.download.textContent = formatRate(data.network.download_bytes_per_sec);
  ids.upload.textContent = formatRate(data.network.upload_bytes_per_sec);

  ids.upsSource.textContent = `Source: ${data.ups.source}`;
  ids.upsStatus.textContent = data.ups.status;
  ids.upsBattery.textContent = `${Math.round(data.ups.battery_percent)}%`;
  ids.upsLoad.textContent = `${Math.round(data.ups.load_percent)}%`;
  ids.upsLinev.textContent = `${Math.round(data.ups.line_voltage)}V`;
  ids.upsRuntime.textContent = `${Math.round(data.ups.runtime_minutes)} min`;
  ids.upsLastxfer.textContent = data.ups.last_transfer;

  pushHistoryPoint(history.temperature, data.system.temperature_c || 0);
  pushHistoryPoint(history.download, data.network.download_bytes_per_sec || 0);
  pushHistoryPoint(history.upload, data.network.upload_bytes_per_sec || 0);

  drawSingleTrend(ids.tempTrend, history.temperature, "#b42318");
  drawDualTrend(ids.networkTrend, history.download, history.upload);
}

async function loadDashboard() {
  try {
    const response = await fetch("/api/dashboard", { cache: "no-store" });
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }

    const data = await response.json();
    render(data);
    lastSuccess = Date.now();
    ids.warning.textContent = "";
    secondsRemaining = 30;
  } catch (error) {
    const now = Date.now();
    if (lastSuccess && now - lastSuccess > 90_000) {
      applyStatus("Stale");
      ids.warning.textContent = "Data is stale. Waiting for successful refresh.";
    } else {
      ids.warning.textContent = "Refresh failed. Retrying at next interval.";
    }
  }
}

setInterval(() => {
  secondsRemaining -= 1;
  if (secondsRemaining <= 0) {
    loadDashboard();
  }
  ids.refreshIn.textContent = `Next refresh: ${Math.max(0, secondsRemaining)}s`;
}, 1000);

loadDashboard();
