const { contextBridge, ipcRenderer } = require('electron')

const bridge = ipcRenderer.sendSync('fastdict:bridge') || {
  base_url: '',
  token: '',
  data_dir: '',
  local_only: true,
}

contextBridge.exposeInMainWorld('__FASTDICT_DESKTOP__', {
  base_url: bridge.base_url || '',
  token: bridge.token || '',
  data_dir: bridge.data_dir || '',
  local_only: Boolean(bridge.local_only),
  pickImportFolders: () => ipcRenderer.invoke('fastdict:pick-folders'),
  pickImportFiles: () => ipcRenderer.invoke('fastdict:pick-files'),
})
