const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("grokhubStop", {
  stop: () => ipcRenderer.invoke("computer:userStop"),
});
