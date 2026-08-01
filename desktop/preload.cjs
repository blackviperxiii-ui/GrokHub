const { contextBridge, ipcRenderer } = require("electron");

/**
 * Exposed to renderer. Host + Grok + Updates talk to main (unsandboxed session).
 */
contextBridge.exposeInMainWorld("grokhubDesktop", {
  minimize: () => ipcRenderer.invoke("desktop:minimize"),
  maximize: () => ipcRenderer.invoke("desktop:maximize"),
  close: () => ipcRenderer.invoke("desktop:close"),
  fit: () => ipcRenderer.invoke("desktop:fit"),
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
    readOpenClawWorkspace: (p) => ipcRenderer.invoke("host:readOpenClawWorkspace", p),
  },
  grok: {
    chat: (payload) => ipcRenderer.invoke("grok:chat", payload),
    imagine: (payload) => ipcRenderer.invoke("grok:imagine", payload),
    probe: (apiKey, accessToken) => ipcRenderer.invoke("grok:probe", apiKey, accessToken),
    oauthStart: () => ipcRenderer.invoke("grok:oauthStart"),
    oauthPoll: (deviceCode) => ipcRenderer.invoke("grok:oauthPoll", deviceCode),
    oauthEnsure: (tokens) => ipcRenderer.invoke("grok:oauthEnsure", tokens),
    checkUpdate: (opts) => ipcRenderer.invoke("update:check", opts),
    applyUpdate: (opts) => ipcRenderer.invoke("update:apply", opts),
    linkWebsiteSession: () => ipcRenderer.invoke("grok:linkWebsiteSession"),
    getWebsiteSso: () => ipcRenderer.invoke("grok:getWebsiteSso"),
    websiteUsage: (opts) => ipcRenderer.invoke("grok:websiteUsage", opts),
  },
});
