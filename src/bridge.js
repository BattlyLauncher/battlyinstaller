const invoke = window.__TAURI__.invoke;
const listen = window.__TAURI__.event.listen;

window.bridge = {
    chooseDirectory: () => invoke('choose_directory'),
    checkAdmin: () => invoke('check_admin'),
    elevateIfNeeded: (needAdmin, state) => invoke('elevate_if_needed', { needAdmin, state }),
    getStartupArgs: () => invoke('get_startup_args'),
    getArchitecture: () => invoke('get_architecture'),
    runUpdate: () => invoke('run_update'),
    getLatestVersion: () => invoke('get_latest_version'),
    getInstallerConfig: () => invoke('get_installer_config'),
    isUninstallMode: () => invoke('is_uninstall_mode'),
    isSilentMode: () => invoke('is_silent_mode'),
    isRunning: (exeName) => invoke('is_running', { exeName }),
    isRunningByPid: (pid) => invoke('is_running_by_pid', { pid }),
    killProcess: (exeName) => invoke('kill_process', { exeName }),
    getPaths: (targetName) => invoke('get_paths', { targetName }),
    detectExisting: (appId) => invoke('detect_existing', { appId }),
    doInstall: (payload) => invoke('do_install', { payload }),
    writeUninstaller: (info) => invoke('write_uninstaller', { info }),
    openInExplorer: (p) => invoke('open_in_explorer', { p }),
    launchApp: (info) => invoke('launch_app', { info }),
    windowMinimize: () => invoke('window_minimize'),
    windowMaximize: () => invoke('window_maximize'),
    windowClose: () => invoke('window_close')
};

// Listen for progress events
listen('install-progress', (event) => {
    const ev = new CustomEvent('install-progress', { detail: event.payload });
    window.dispatchEvent(ev);
});
