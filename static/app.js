const ids = {
  globalStatus: document.getElementById("global-status"),
  lastUpdated: document.getElementById("last-updated"),
  refreshIn: document.getElementById("refresh-in"),
  warning: document.getElementById("warning"),
  cpu: document.getElementById("cpu"),
  cpuDetail: document.getElementById("cpu-detail"),
  ram: document.getElementById("ram"),
  ramDetail: document.getElementById("ram-detail"),
  storage: document.getElementById("storage"),
  storageDetail: document.getElementById("storage-detail"),
  temp: document.getElementById("temp"),
  tempF: document.getElementById("temp-f"),
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

let lastSuccess = null;
const MAX_HISTORY = 20;
const STALE_AFTER_MS = 15_000;

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
    <defs>
      <filter id="temp-glow" x="-30%" y="-80%" width="160%" height="260%">
        <feGaussianBlur stdDeviation="2.5" result="coloredBlur" />
        <feMerge>
          <feMergeNode in="coloredBlur" />
          <feMergeNode in="SourceGraphic" />
        </feMerge>
      </filter>
    </defs>
    <line x1="0" y1="${height}" x2="${width}" y2="${height}" stroke="#2a3352" stroke-width="1" />
    <polyline fill="none" stroke="${strokeColor}" stroke-width="2.5" points="${points}" filter="url(#temp-glow)" />
  `;
}

function drawDualTrend(svg, firstValues, secondValues) {
  const width = 900;
  const height = 110;
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
    <defs>
      <filter id="network-glow" x="-20%" y="-60%" width="140%" height="220%">
        <feGaussianBlur stdDeviation="2" result="coloredBlur" />
        <feMerge>
          <feMergeNode in="coloredBlur" />
          <feMergeNode in="SourceGraphic" />
        </feMerge>
      </filter>
    </defs>
    <line x1="0" y1="${height}" x2="${width}" y2="${height}" stroke="#2a3352" stroke-width="1" />
    <polyline fill="none" stroke="#ff2ea6" stroke-width="3" points="${downPoints}" filter="url(#network-glow)" />
    <polyline fill="none" stroke="#00f7ff" stroke-width="2.4" points="${upPoints}" filter="url(#network-glow)" />
  `;
}

function formatRate(bytesPerSecond) {
  const kb = bytesPerSecond / 1024;
  if (kb < 1024) {
    return `${kb.toFixed(0)} KB/s`;
  }
  return `${(kb / 1024).toFixed(2)} MB/s`;
}

function formatMb(bytes) {
  const mb = (Math.max(0, Number(bytes) || 0) / (1024 ** 2));
  return `${mb.toFixed(0)} MB`;
}

function formatGb(bytes) {
  const gb = (Math.max(0, Number(bytes) || 0) / (1024 ** 3));
  return `${gb.toFixed(1)} GB`;
}

function setUsageClass(element, value, warningThreshold, criticalThreshold) {
  element.classList.remove("usage-normal", "usage-warning", "usage-critical");
  if (value >= criticalThreshold) {
    element.classList.add("usage-critical");
  } else if (value >= warningThreshold) {
    element.classList.add("usage-warning");
  } else {
    element.classList.add("usage-normal");
  }
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
  ids.cpuDetail.textContent = `${data.system.cpu_active_cores} Cores/${data.system.cpu_total_cores} Cores`;
  ids.ram.textContent = data.system.ram_percent;
  ids.ramDetail.textContent = `${formatMb(data.system.ram_used_bytes)}/${formatMb(data.system.ram_total_bytes)}`;
  ids.storage.textContent = data.system.storage_percent;
  ids.storageDetail.textContent = `${formatGb(data.system.storage_used_bytes)}/${formatGb(data.system.storage_total_bytes)}`;
  ids.temp.textContent = data.system.temperature_c.toFixed(1);
  ids.tempF.textContent = `(${((data.system.temperature_c * 9) / 5 + 32).toFixed(1)} F)`;

  setUsageClass(ids.cpu, data.system.cpu_percent, 70, 90);
  setUsageClass(ids.ram, data.system.ram_percent, 70, 90);
  setUsageClass(ids.storage, data.system.storage_percent, 75, 92);
  setUsageClass(ids.temp, data.system.temperature_c, 60, 70);

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

  drawSingleTrend(ids.tempTrend, history.temperature, "#ffd43b");
  drawDualTrend(ids.networkTrend, history.download, history.upload);
}

function handleSnapshot(data) {
  render(data);
  lastSuccess = Date.now();
  ids.warning.textContent = "";
}

async function fetchSnapshotFallback() {
  try {
    const response = await fetch("/api/dashboard", { cache: "no-store" });
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }
    handleSnapshot(await response.json());
  } catch (_) {
    ids.warning.textContent = "Refresh failed. Retrying.";
  }
}

function connectStream() {
  const source = new EventSource("/api/dashboard/stream");

  source.onmessage = (event) => {
    try {
      handleSnapshot(JSON.parse(event.data));
    } catch (_) {
      // ignore malformed frame; next frame will recover
    }
  };

  source.onerror = () => {
    ids.warning.textContent = "Live stream interrupted. Reconnecting.";
    // EventSource reconnects automatically; nothing else to do.
  };
}

setInterval(() => {
  const now = Date.now();
  if (!lastSuccess) {
    ids.refreshIn.textContent = "Connecting...";
    return;
  }
  const age = Math.max(0, Math.round((now - lastSuccess) / 1000));
  ids.refreshIn.textContent = `Updated ${age}s ago`;
  if (now - lastSuccess > STALE_AFTER_MS) {
    applyStatus("Stale");
    ids.warning.textContent = "Data is stale. Waiting for next sample.";
  }
}, 1000);

fetchSnapshotFallback();
connectStream();
