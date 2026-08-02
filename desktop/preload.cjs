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
    exec: (command, cwd, timeoutMs, opts) =>
      ipcRenderer.invoke("host:exec", command, cwd, timeoutMs, opts),
    killExec: (jobId) => ipcRenderer.invoke("host:killExec", jobId),
    setSafeMode: (enabled) => ipcRenderer.invoke("host:setSafeMode", enabled),
    getSafeMode: () => ipcRenderer.invoke("host:getSafeMode"),
    listApps: () => ipcRenderer.invoke("host:listApps"),
    openApp: (opts) => ipcRenderer.invoke("host:openApp", opts),
    readOpenClawWorkspace: (p) => ipcRenderer.invoke("host:readOpenClawWorkspace", p),
  },
  grok: {
    chat: (payload) => ipcRenderer.invoke("grok:chat", payload),
    chatStream: (payload, handlers) => {
      // Streaming via main is event-based; handlers optional for future
      return ipcRenderer.invoke("grok:chatStream", payload);
    },
    imagine: (payload) => ipcRenderer.invoke("grok:imagine", payload),
    transcribe: (payload) => ipcRenderer.invoke("grok:transcribe", payload),
    probe: (apiKey, accessToken) => ipcRenderer.invoke("grok:probe", apiKey, accessToken),
    oauthStart: () => ipcRenderer.invoke("grok:oauthStart"),
    oauthPoll: (deviceCode) => ipcRenderer.invoke("grok:oauthPoll", deviceCode),
    oauthEnsure: (tokens) => ipcRenderer.invoke("grok:oauthEnsure", tokens),
    checkUpdate: (opts) => ipcRenderer.invoke("update:check", opts),
    applyUpdate: (opts) => ipcRenderer.invoke("update:apply", opts),
    checkRollback: (opts) => ipcRenderer.invoke("update:checkRollback", opts),
    applyRollback: (opts) => ipcRenderer.invoke("update:rollback", opts),
    selfTest: (opts) => ipcRenderer.invoke("update:selfTest", opts),
    factoryReinstall: (opts) => ipcRenderer.invoke("update:factory", opts),
    linkWebsiteSession: () => ipcRenderer.invoke("grok:linkWebsiteSession"),
    getWebsiteSso: () => ipcRenderer.invoke("grok:getWebsiteSso"),
    injectWebsiteCookie: (raw) => ipcRenderer.invoke("grok:injectWebsiteCookie", raw),
    websiteUsage: (opts) => ipcRenderer.invoke("grok:websiteUsage", opts),
    websiteConnectors: (opts) => ipcRenderer.invoke("grok:websiteConnectors", opts),
  },
  secrets: {
    set: (key, value) => ipcRenderer.invoke("secrets:set", key, value),
    get: (key) => ipcRenderer.invoke("secrets:get", key),
    delete: (key) => ipcRenderer.invoke("secrets:delete", key),
  },
  state: {
    get: (name) => ipcRenderer.invoke("state:get", name),
    set: (name, value) => ipcRenderer.invoke("state:set", name, value),
    remove: (name) => ipcRenderer.invoke("state:remove", name),
    info: () => ipcRenderer.invoke("state:info"),
    exportAll: () => ipcRenderer.invoke("state:export"),
    importAll: (payload) => ipcRenderer.invoke("state:import", payload),
  },
  selfmod: {
    info: () => ipcRenderer.invoke("selfmod:info"),
    list: (rel) => ipcRenderer.invoke("selfmod:list", rel),
    read: (rel) => ipcRenderer.invoke("selfmod:read", rel),
    write: (rel, content, opts) => ipcRenderer.invoke("selfmod:write", rel, content, opts),
    patch: (rel, find, replace, opts) =>
      ipcRenderer.invoke("selfmod:patch", rel, find, replace, opts),
    snapshot: (note) => ipcRenderer.invoke("selfmod:snapshot", note),
    restore: (id) => ipcRenderer.invoke("selfmod:restore", id),
    journal: (limit) => ipcRenderer.invoke("selfmod:journal", limit),
  },
  desktopEntry: {
    status: () => ipcRenderer.invoke("desktopEntry:status"),
    install: (opts) => ipcRenderer.invoke("desktopEntry:install", opts),
    autostart: (enabled) => ipcRenderer.invoke("desktopEntry:autostart", enabled),
  },
  imagineMedia: {
    save: (jobId, dataUrl, kind) => ipcRenderer.invoke("imagine:save", jobId, dataUrl, kind),
    load: (relPath) => ipcRenderer.invoke("imagine:load", relPath),
    delete: (jobId) => ipcRenderer.invoke("imagine:delete", jobId),
    clear: () => ipcRenderer.invoke("imagine:clear"),
  },
});
