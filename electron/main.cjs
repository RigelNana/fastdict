const { app, BrowserWindow, dialog, ipcMain, Menu, nativeTheme, shell } = require('electron')
const { spawn } = require('node:child_process')
const crypto = require('node:crypto')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const readline = require('node:readline')

if (process.platform === 'linux') {
  app.commandLine.appendSwitch('ozone-platform-hint', 'auto')
  app.commandLine.appendSwitch('enable-features', 'WaylandWindowDecorations')
}

let mainWindow = null
let backendProcess = null
let quitting = false

function resolveDataDir() {
  const custom = process.env.FASTDICT_DATA_DIR?.trim()
  if (custom) return custom
  if (process.platform === 'linux') {
    const xdgData = process.env.XDG_DATA_HOME || path.join(os.homedir(), '.local', 'share')
    return path.join(xdgData, 'fastdict')
  }
  return path.join(app.getPath('userData'), 'data')
}

function resolveBinaryPath() {
  const binName = process.platform === 'win32' ? 'fastdict.exe' : 'fastdict'
  if (app.isPackaged) {
    return path.join(process.resourcesPath, 'bin', binName)
  }
  return path.join(__dirname, '..', 'target', 'release', binName)
}

function resolveStaticDir() {
  if (app.isPackaged) {
    return path.join(process.resourcesPath, 'dist')
  }
  return path.join(__dirname, '..', 'dist')
}

function localImportRoots() {
  if (process.platform !== 'win32') return ['/']
  const roots = []
  for (let code = 65; code <= 90; code++) {
    const drive = `${String.fromCharCode(code)}:\\`
    if (fs.existsSync(drive)) roots.push(drive)
  }
  return roots
}

function startBackend(dataDir, token) {
  return new Promise((resolve, reject) => {
    const binary = resolveBinaryPath()
    const staticDir = resolveStaticDir()
    if (!fs.existsSync(binary)) {
      reject(new Error(`未找到内置词典服务程序：${binary}`))
      return
    }
    fs.mkdirSync(dataDir, { recursive: true })
    const args = [
      'serve',
      '--listen',
      '127.0.0.1:0',
      '--data-dir',
      dataDir,
      '--static-dir',
      staticDir,
    ]
    for (const root of localImportRoots()) {
      args.push('--import-root', root)
    }

    const child = spawn(binary, args, {
      env: { ...process.env, FASTDICT_TOKEN: token },
      stdio: ['ignore', 'pipe', 'pipe'],
      windowsHide: true,
    })
    backendProcess = child

    let settled = false
    let stderrTail = ''

    const rl = readline.createInterface({ input: child.stdout })
    rl.on('line', (line) => {
      const match = line.match(/^FASTDICT_READY\s+(http:\/\/127\.0\.0\.1:\d+)/)
      if (match && !settled) {
        settled = true
        rl.close()
        resolve(match[1])
      }
    })

    child.stderr.on('data', (chunk) => {
      stderrTail = (stderrTail + chunk.toString()).slice(-2000)
    })

    child.once('error', (err) => {
      if (!settled) {
        settled = true
        reject(err)
      }
    })

    child.once('exit', (code, signal) => {
      if (backendProcess === child) backendProcess = null
      if (!settled) {
        settled = true
        reject(
          new Error(
            `内置词典服务意外退出 (code=${code}, signal=${signal})\n${stderrTail}`.trim(),
          ),
        )
      }
    })
  })
}

function stopBackend() {
  const child = backendProcess
  backendProcess = null
  if (!child || child.killed) return
  try {
    child.kill('SIGTERM')
    setTimeout(() => {
      if (!child.killed) {
        try {
          child.kill('SIGKILL')
        } catch {
          /* already exited */
        }
      }
    }, 2000).unref()
  } catch {
    /* ignore */
  }
}

function setupMenu() {
  if (process.platform === 'darwin') {
    const template = [
      { role: 'appMenu' },
      { role: 'editMenu' },
      { role: 'viewMenu' },
      { role: 'windowMenu' },
    ]
    Menu.setApplicationMenu(Menu.buildFromTemplate(template))
  } else {
    Menu.setApplicationMenu(null)
  }
}

function createWindow() {
  const iconPath = path.join(
    app.isPackaged ? process.resourcesPath : path.join(__dirname, '..', 'build'),
    'icon.png',
  )
  const win = new BrowserWindow({
    width: 1200,
    height: 820,
    minWidth: 760,
    minHeight: 560,
    title: 'FastDict',
    icon: fs.existsSync(iconPath) ? iconPath : undefined,
    backgroundColor: nativeTheme.shouldUseDarkColors ? '#09090b' : '#ffffff',
    autoHideMenuBar: true,
    show: true,
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  })

  win.webContents.setWindowOpenHandler(({ url }) => {
    if (/^https?:\/\//i.test(url)) {
      void shell.openExternal(url)
    }
    return { action: 'deny' }
  })

  win.on('closed', () => {
    if (mainWindow === win) mainWindow = null
  })

  mainWindow = win
  return win
}

const gotLock = app.requestSingleInstanceLock()
if (!gotLock) {
  app.quit()
} else {
  app.on('second-instance', () => {
    if (!mainWindow) return
    if (mainWindow.isMinimized()) mainWindow.restore()
    mainWindow.focus()
  })

  ipcMain.handle('fastdict:pick-folders', async () => {
    const owner = mainWindow ?? undefined
    const result = await dialog.showOpenDialog(owner, {
      title: '选择一个或多个词典文件夹',
      properties: ['openDirectory', 'multiSelections'],
    })
    return result.canceled ? [] : result.filePaths
  })

  ipcMain.handle('fastdict:pick-files', async () => {
    const owner = mainWindow ?? undefined
    const result = await dialog.showOpenDialog(owner, {
      title: '选择词典与伴随资源文件',
      properties: ['openFile', 'multiSelections'],
    })
    return result.canceled ? [] : result.filePaths
  })

  app.whenReady().then(async () => {
    setupMenu()
    const dataDir = resolveDataDir()
    const token = crypto.randomBytes(32).toString('hex')
    const bridge = {
      base_url: '',
      token,
      data_dir: dataDir,
      local_only: true,
    }
    ipcMain.on('fastdict:bridge', (event) => {
      event.returnValue = bridge
    })

    // Open window immediately for instant perceived startup while Rust engine initializes in parallel.
    const win = createWindow()

    try {
      const serverUrl = await startBackend(dataDir, token)
      if (!win.isDestroyed()) {
        await win.loadURL(serverUrl)
      }
    } catch (error) {
      dialog.showErrorBox(
        'FastDict 启动失败',
        error instanceof Error ? error.message : String(error),
      )
      app.quit()
    }
  })

  app.on('before-quit', () => {
    quitting = true
    stopBackend()
  })

  app.on('window-all-closed', () => {
    stopBackend()
    if (process.platform !== 'darwin' || quitting) {
      app.quit()
    }
  })
}
