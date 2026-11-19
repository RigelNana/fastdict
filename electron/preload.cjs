const { contextBridge, ipcRenderer } = require('electron')

const arg = process.argv.find((item) => item.startsWith('--fastdict-bridge='))
let bridge = { base_url: '', token: '', data_dir: '', local_only: true }
if (arg) {
  try {
    bridge = JSON.parse(Buffer.from(arg.slice('--fastdict-bridge='.length), 'base64').toString('utf8'))
  } catch {
    /* ignore malformed bridge arg */
  }
}

contextBridge.exposeInMainWorld('__FASTDICT_DESKTOP__', {
  base_url: bridge.base_url,
  token: bridge.token,
  data_dir: bridge.data_dir,
  local_only: Boolean(bridge.local_only),
  pickImportFolders: () => ipcRenderer.invoke('fastdict:pick-folders'),
  pickImportFiles: () => ipcRenderer.invoke('fastdict:pick-files'),
})
