const { contextBridge, ipcRenderer } = require("electron");

/**
 * Exposed to renderer. Host methods talk to main process (unsandboxed user session).
 */
contextBridge.exposeInMainWorld("grokclawDesktop", {
  minimize: () => ipcRenderer.invoke("desktop:minimize"),
  maximize: () => ipcRenderer.invoke("desktop:maximize"),
  close: () => ipcRenderer.invoke("desktop:close"),
  platform: process.platform,
  host: {
    info: () => ipcRenderer.invoke("host:info"),
    listDir: (p) => ipcRenderer.invoke("host:listDir", p),
    readFile: (p, maxBytes) => ipcRenderer.invoke("host:readFile", p, maxBytes),
    writeFile: (p, content) => ipcRenderer.invoke("host:writeFile", p, content),
    exec: (command, cwd, timeoutMs) =>
      ipcRenderer.invoke("host:exec", command, cwd, timeoutMs),
    listApps: () => ipcRenderer.invoke("host:listApps"),
    openApp: (opts) => ipcRenderer.invoke("host:openApp", opts),
  },
});
