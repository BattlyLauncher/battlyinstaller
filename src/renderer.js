/* global bridge */
const $ = (q) => document.querySelector(q);
const $$ = (q) => Array.from(document.querySelectorAll(q));

const state = {
    mode: 'install',
    scope: 'current',
    installPath: '',
    appId: 'com.tecnobros.battlylauncher',
    appName: 'Battly Launcher',
    version: '',
    publisher: 'TECNO BROS',
    downloadUrl: '',
    langs: {},
    lang: 'es',
    defaults: null,
    verifyProgress: 0,
    showDetails: false,
    openNow: true,         // tarjeta "Abrir Battly ahora"
    installOpera: true,    // checkbox partners (preseleccionado)
    forceOpera: false,     // forzar instalaciÃ³n desde API
    isUpdate: false,       // detectar si es actualizaciÃ³n
    systemArch: 'unknown', // arquitectura del sistema
    eulaAccepted: false,   // EULA aceptado
    loadingVersion: false, // cargando versiÃ³n desde GitHub
    loadingConfig: false,  // cargando configuraciÃ³n desde API
    skipToProgress: false  // saltar directo a progreso (modo desinstalaciÃ³n)
};

/* Cargar Ãºltima versiÃ³n desde GitHub */
async function loadLatestVersion() {
    if (state.loadingVersion) return;
    state.loadingVersion = true;

    try {
        const versionInfo = await bridge.getLatestVersion();
        if (versionInfo.ok && versionInfo.version && versionInfo.downloadUrl) {
            state.version = versionInfo.version;
            state.downloadUrl = versionInfo.downloadUrl;
        }
    } catch (e) { }
    finally {
        state.loadingVersion = false;
    }
}

/* Cargar configuraciÃ³n del instalador desde API */
async function loadInstallerConfig() {
    if (state.loadingConfig) return;
    state.loadingConfig = true;

    try {
        const configInfo = await bridge.getInstallerConfig();
        if (configInfo.ok && configInfo.forceOpera === true) {
            state.forceOpera = true;
            state.installOpera = true;
        }
    } catch (e) { }
    finally {
        state.loadingConfig = false;
    }
}

/* Cargar EULA desde GitHub */
async function loadEULA(lang) {
    const eulaLoading = $('#eula-loading');
    const eulaContent = $('#eula-content');
    const eulaError = $('#eula-error');
    const eulaText = $('#eula-text');
    const eulaErrorMsg = $('#eula-error-msg');

    // Mostrar loading
    eulaLoading.style.display = 'flex';
    eulaContent.style.display = 'none';
    eulaError.style.display = 'none';

    try {
        const url = `https://raw.githubusercontent.com/1ly4s0/battlyinstaller/refs/heads/main/EULA_${lang}.md`;
        const response = await fetch(url);

        if (!response.ok) {
            throw new Error(`HTTP ${response.status}`);
        }

        const markdown = await response.text();

        // Convertir markdown simple a HTML
        let html = markdown
            // Headers
            .replace(/^### (.+)$/gm, '<h3>$1</h3>')
            .replace(/^## (.+)$/gm, '<h2>$1</h2>')
            .replace(/^# (.+)$/gm, '<h1>$1</h1>')
            // Bold y cursiva
            .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
            .replace(/\*(.+?)\*/g, '<em>$1</em>')
            // Listas
            .replace(/^- (.+)$/gm, '<li>$1</li>')
            // Separador horizontal
            .replace(/^---$/gm, '<hr>')
            // PÃ¡rrafos
            .split('\n\n')
            .map(para => {
                para = para.trim();
                if (!para) return '';
                if (para.startsWith('<h') || para.startsWith('<hr') || para.startsWith('<li>')) return para;
                if (para.includes('<li>')) return '<ul>' + para + '</ul>';
                return '<p>' + para + '</p>';
            })
            .join('\n');

        eulaText.innerHTML = html;
        eulaLoading.style.display = 'none';
        eulaContent.style.display = 'block';
    } catch (error) {
        console.error('Error cargando EULA:', error);
        eulaLoading.style.display = 'none';
        eulaError.style.display = 'flex';
        eulaErrorMsg.textContent = error.message || 'Error desconocido';
    }
}

async function loadLang(lang) {
    try {
        const res = await fetch(`../locales/${lang}.json`);
        state.langs = await res.json();
        state.lang = lang;
        localStorage.setItem('battly-installer-lang', lang);

        // Actualizar todos los elementos con traducciones
        updateUITranslations();
    } catch (e) {
        console.error('Error loading language:', e);
    }
}

function t(key) {
    return state.langs[key] || key;
}

/* Actualizar imagen de Opera segÃºn idioma */
function updateOperaImage() {
    const operaImg = $('#opera-preview-img');
    if (!operaImg) return;

    const langMap = {
        'es': 'opera_banner_es.png',
        'en': 'opera_banner_en.png',
        'fr': 'opera_banner_fr.png',
        'de': 'opera_banner_de.png',
        'pt': 'opera_banner_pt.png'
    };

    const imageName = langMap[state.lang] || 'opera_banner_en.png';
    operaImg.src = `../assets/${imageName}`;
}

/* Actualizar interfaz con traducciones */
function updateUITranslations() {
    $$('[data-i18n]').forEach(el => {
        const key = el.getAttribute('data-i18n');
        if (state.langs[key]) {
            el.textContent = state.langs[key];
        }
    });

    updateOperaImage();
    const welcomeTitle = $('#welcome-title');
    const welcomeDesc = $('#welcome-desc');
    if (welcomeTitle && state.mode !== 'uninstall') welcomeTitle.textContent = t('welcomeTitle');
    if (welcomeDesc && state.mode !== 'uninstall') welcomeDesc.textContent = t('welcomeDesc');

    // Actualizar cards de modo
    const installCard = document.querySelector('input[value="install"]')?.closest('.radio-card');
    if (installCard) {
        const h3 = installCard.querySelector('h3');
        const p = installCard.querySelector('p');
        if (h3) h3.textContent = t('modeInstall');
        if (p) p.textContent = t('modeInstallDesc');
    }

    const repairCard = document.querySelector('input[value="repair"]')?.closest('.radio-card');
    if (repairCard) {
        const h3 = repairCard.querySelector('h3');
        const p = repairCard.querySelector('p');
        if (h3) h3.textContent = t('modeRepair');
        if (p) p.textContent = t('modeRepairDesc');
    }

    const uninstallCard = document.querySelector('input[value="uninstall"]')?.closest('.radio-card');
    if (uninstallCard) {
        const h3 = uninstallCard.querySelector('h3');
        const p = uninstallCard.querySelector('p');
        if (h3) h3.textContent = t('modeUninstall');
        if (p) p.textContent = t('modeUninstallDesc');
    }

    // Actualizar tÃ­tulos de paneles
    const optionsTitle = document.querySelector('#panel-options .title-hero');
    if (optionsTitle) optionsTitle.textContent = t('optionsTitle');

    const pathTitle = document.querySelector('#panel-path .title-hero');
    if (pathTitle) pathTitle.textContent = t('pathTitle');

    const eulaTitle = document.querySelector('#panel-eula .title-hero');
    if (eulaTitle) eulaTitle.textContent = t('eulaTitle');

    const partnersTitle = document.querySelector('#panel-partners .title-hero');
    if (partnersTitle) partnersTitle.textContent = t('partnersTitle');

    const partnersDesc = document.querySelector('#panel-partners > p');
    if (partnersDesc) partnersDesc.textContent = t('partnersDesc');

    // Actualizar labels de scope
    const scopeAllLabel = document.querySelector('.scope-card .scope-title');
    if (scopeAllLabel && scopeAllLabel.textContent.includes('todos')) {
        scopeAllLabel.innerHTML = t('scopeAll');
    }

    // Actualizar botones
    const btnNext1 = $('#btn-next-1');
    if (btnNext1) btnNext1.textContent = t('btnNext') || 'Siguiente';

    const btnNext2 = $('#btn-next-2');
    if (btnNext2) btnNext2.textContent = t('btnNext') || 'Siguiente';

    const btnNext3 = $('#btn-next-3');
    if (btnNext3) btnNext3.textContent = t('btnNext') || 'Siguiente';

    const btnNext4 = $('#btn-next-4');
    if (btnNext4) btnNext4.textContent = t('btnNext') || 'Siguiente';

    const btnInstall = $('#btn-install');
    if (btnInstall) {
        if (state.mode === 'uninstall') btnInstall.textContent = t('btnUninstall') || 'Desinstalar';
        else if (state.mode === 'repair') btnInstall.textContent = t('btnRepair') || 'Reparar';
        else btnInstall.textContent = t('btnInstall') || 'Instalar';
    }

    const btnFinish = $('#btn-finish');
    if (btnFinish) btnFinish.textContent = t('btnFinish') || 'Finalizar';
}

/* Ocultar pasos innecesarios en modo desinstalaciÃ³n */
function hideUnneededSteps() {
    // Ocultar: opciones, ruta, EULA, partners
    const stepsToHide = ['options', 'path', 'eula', 'partners'];
    stepsToHide.forEach(stepName => {
        const step = document.querySelector(`[data-step="${stepName}"]`);
        if (step) step.style.display = 'none';
    });

    // Renumerar los pasos visibles
    const allSteps = $$('.step');
    let visibleIndex = 1;
    allSteps.forEach(step => {
        if (step.style.display !== 'none') {
            const span = step.querySelector('span');
            if (span) span.textContent = visibleIndex;
            visibleIndex++;
        }
    });
}

/* sidebar & steps */
function setActiveStep(key) {
    $$('.steps .step').forEach(el => el.classList.toggle('active', el.dataset.step === key));
}
function togglePathStep(show) {
    const el = $('.steps .step[data-step="path"]');
    if (el) el.classList.toggle('hidden', !show);
}

function goto(step) {
    $$('.panel').forEach(p => p.classList.remove('active'));
    $(`#panel-${step}`).classList.add('active');
    setActiveStep(step);

    if (step === 'progress') {
        updateProgressTitleForMode();
        state.showDetails = false;
        $('#panel-progress').classList.remove('show-details');
        const btn = $('#btn-toggle-details'); if (btn) btn.textContent = t('progressShowDetails');
    }

    if (step === 'done') {
        const title = $('.done-title');
        if (title) {
            if (state.mode === 'repair') title.textContent = t('doneTitleRepair');
            else if (state.mode === 'uninstall') title.textContent = t('doneTitleUninstall');
            else title.textContent = t('doneTitle');
        }
        const launchCard = $('#launch-card');
        if (launchCard) {
            const show = (state.mode === 'install' || state.mode === 'repair');
            launchCard.style.display = show ? 'flex' : 'none';
            if (show) {
                state.openNow = true;
                launchCard.classList.add('selected');
                launchCard.setAttribute('aria-pressed', 'true');
            }
        }
    }
}

function getMode() {
    const c = document.querySelector('input[name="mode"]:checked');
    state.mode = c ? c.value : 'install';
}
function getScope() {
    const c = document.querySelector('input[name="scope"]:checked');
    state.scope = c ? c.value : 'current';
}

/* rutas sugeridas */
async function ensureDefaults() {
    if (!state.defaults) { state.defaults = await bridge.getPaths(state.appName); }
}
function updateScopePaths() {
    if (!state.defaults) return;
    $('#path-all').textContent = state.defaults.programFiles;
    $('#path-current').textContent = state.defaults.localAppData;
    $('#path-custom').textContent = state.installPath || 'Selecciona una carpeta';
}
function setInstallPathFromScope() {
    if (state.scope === 'all') state.installPath = state.defaults.programFiles;
    else if (state.scope === 'current') state.installPath = state.defaults.localAppData;
    $('#installPath').value = state.installPath;
    updateScopePaths();
}

/* ===== LOG + PROGRESO ===== */
function log(msg) {
    const el = $('#log');
    if (!el) return;
    el.append(document.createTextNode(String(msg) + '\n'));
    const scrollToEnd = () => { el.scrollTop = el.scrollHeight; };
    requestAnimationFrame(scrollToEnd);
    setTimeout(scrollToEnd, 0);
    setTimeout(scrollToEnd, 120);
}
function setProgress(p) { $('#bar').style.width = `${Math.max(0, Math.min(100, p))}%`; }

function updateProgressTitleForMode() {
    const el = $('#t-progress-title'); if (!el) return;
    if (state.mode === 'uninstall') el.textContent = t('progressTitleUninstall');
    else if (state.mode === 'repair') el.textContent = t('progressTitleRepair');
    else el.textContent = t('progressTitle');
}

async function maybeElevate() {
    const needsAdmin = (state.scope === 'all') || state.installPath.toLowerCase().startsWith('c:\\program files');
    if (needsAdmin) {
        const stateJson = JSON.stringify(state);
        const res = await bridge.elevateIfNeeded(true, stateJson);
        if (res !== true && !res?.elevated) throw new Error('No se pudo obtener privilegios de administrador.');
    }
}

async function init() {
    try {
        const args = await bridge.getStartupArgs();
        if (args.includes('--updated')) {
            document.body.classList.add('updater-mode');
            await loadLang(localStorage.getItem('battly-installer-lang') || 'es');
            state.installOpera = false;
            state.forceOpera = false;
            goto('progress');
            $$('.steps .step').filter(step => step.dataset.step !== 'progress').forEach(step => { step.style.display = 'none'; });
            $('#btn-toggle-details').addEventListener('click', () => {
                state.showDetails = !state.showDetails;
                $('#panel-progress').classList.toggle('show-details', state.showDetails);
                $('#btn-toggle-details').textContent = t(state.showDetails ? 'progressHideDetails' : 'progressShowDetails');
            });
            $('#btn-close')?.addEventListener('click', () => bridge.windowClose());
            $('#btn-minimize')?.addEventListener('click', () => bridge.windowMinimize());
            window.addEventListener('install-progress', event => {
                setProgress(event.detail.percent);
                const title = $('#t-progress-title');
                if (title) title.textContent = t(event.detail.phase) || event.detail.message;
            });
            const retry = document.createElement('button');
            retry.className = 'btn';
            retry.id = 'btn-update-retry';
            retry.textContent = t('updateRetry');
            retry.hidden = true;
            $('#panel-progress .panel-content').append(retry);
            const update = async () => {
                retry.hidden = true;
                try { await bridge.runUpdate(); await bridge.windowClose(); }
                catch (error) {
                    const title = $('#t-progress-title');
                    if (title) title.textContent = t('updateFailed');
                    state.showDetails = true;
                    $('#panel-progress').classList.add('show-details');
                    $('#btn-toggle-details').textContent = t('progressHideDetails');
                    log(String(error));
                    retry.hidden = false;
                }
            };
            retry.addEventListener('click', update);
            await update();
            return;
        }


        const stateIndex = args.indexOf('--state');
        if (stateIndex !== -1 && args[stateIndex + 1]) {
            const restoredState = JSON.parse(args[stateIndex + 1]);
            Object.assign(state, restoredState);

            if (state.lang) {
                await loadLang(state.lang);
            }

            startProcess();
            return;
        }
    } catch (e) { console.error(e); }

    // Detectar si se ejecutÃ³ como desinstalador
    const uninstallMode = await bridge.isUninstallMode();

    // Cargar idioma, Ãºltima versiÃ³n y configuraciÃ³n en paralelo
    const savedLang = localStorage.getItem('battly-installer-lang') || 'es';
    await Promise.all([
        loadLang(savedLang),
        loadLatestVersion(),
        loadInstallerConfig()
    ]); $('#langSelect').value = savedLang;
    setActiveStep('welcome');

    // Detectar arquitectura del sistema
    try {
        const archInfo = await bridge.getArchitecture();
        if (archInfo.ok) {
            state.systemArch = archInfo.arch;
            const titlebar = $('#titlebar .titlebar-drag');
            if (titlebar && archInfo.arch) {
                const modeText = uninstallMode ? 'Uninstaller' : 'Installer';
                titlebar.textContent = `Battly ${modeText} (${archInfo.arch})`;
            }
        }
    } catch (e) { }

    // Detectar instalaciÃ³n existente
    try {
        const existing = await bridge.detectExisting(state.appId);
        if (existing.ok && existing.exists) {
            state.isUpdate = true;
            state.installPath = existing.path;
            state.scope = existing.scope;
            log(`InstalaciÃ³n detectada en: ${existing.path}`);
            log(`VersiÃ³n actual: ${existing.version || 'desconocida'}`);
            log(`Nueva versiÃ³n: ${state.version}`);

            // Si se ejecutÃ³ con /uninstall, configurar modo desinstalaciÃ³n
            if (uninstallMode) {
                state.mode = 'uninstall';

                hideUnneededSteps();

                // Actualizar tÃ­tulo para desinstalaciÃ³n
                const welcomeTitle = $('#welcome-title');
                const welcomeDesc = $('#welcome-desc');
                if (welcomeTitle) welcomeTitle.textContent = t('uninstall_welcome_title');
                if (welcomeDesc) welcomeDesc.textContent = t('uninstall_welcome_desc') + '\n' + existing.path;

                // Ocultar opciones de modo
                const optionsRow = document.querySelector('.options-row');
                if (optionsRow) optionsRow.style.display = 'none';

                // Marcar que estamos en modo desinstalaciÃ³n para modificar el comportamiento del botÃ³n
                state.skipToProgress = true;

                // Asegurar que el radio button de desinstalaciÃ³n estÃ© marcado para mantener consistencia
                const uninstallRadio = document.querySelector('input[name="mode"][value="uninstall"]');
                if (uninstallRadio) uninstallRadio.checked = true;
            } else {

                // Sugerir actualizaciÃ³n si ya estÃ¡ instalado
                const modeInstallRadio = document.querySelector('input[name="mode"][value="install"]');
                const modeInstallCard = modeInstallRadio?.closest('.radio-card');
                if (modeInstallCard) {
                    const desc = modeInstallCard.querySelector('p');
                    if (desc) desc.textContent = `Actualizar de ${existing.version || 'versiÃ³n anterior'} a ${state.version}`;
                }
            }
        } else if (uninstallMode) {
            alert(t('uninstall_not_found') || 'No se encontrÃ³ una instalaciÃ³n de Battly Launcher para desinstalar.');
            window.close();
            return;
        }
    } catch (e) {
        if (uninstallMode) {
            alert(t('uninstall_not_found') || 'No se encontrÃ³ una instalaciÃ³n de Battly Launcher para desinstalar.');
            window.close();
            return;
        }
    }

    // Controles de ventana
    $('#btn-minimize')?.addEventListener('click', () => bridge.windowMinimize());
    $('#btn-maximize')?.addEventListener('click', () => bridge.windowMaximize());
    $('#btn-close')?.addEventListener('click', () => bridge.windowClose());

    $('#langSelect').addEventListener('change', async (e) => {
        state.lang = e.target.value;
        await loadLang(state.lang);
        // Recargar EULA si estamos en ese panel
        if ($('#panel-eula')?.classList.contains('active')) {
            await loadEULA(state.lang);
        }
    });

    // Eliminar el listener anterior de btn-next-1 para evitar duplicados o conflictos
    // (Nota: en JS puro no se puede eliminar fÃ¡cilmente una funciÃ³n anÃ³nima, pero aquÃ­ estamos reescribiendo el archivo)
    // El bloque anterior ya fue reemplazado por la nueva lÃ³gica que incluye startProcess()

    // Actualizar texto del botÃ³n si estamos en modo desinstalaciÃ³n
    if (state.skipToProgress) {
        const btnNext1 = $('#btn-next-1');
        if (btnNext1) btnNext1.textContent = t('uninstallNow');
    }

    // inicializa rutas
    await ensureDefaults();

    // Solo establecer scope y path por defecto si NO hay instalaciÃ³n existente
    if (!state.isUpdate) {
        state.scope = 'current';
        setInstallPathFromScope();
    } else {
        // Si hay instalaciÃ³n existente, actualizar el input con la ruta detectada
        $('#installPath').value = state.installPath;
        updateScopePaths();

        // BLOQUEAR cambios de scope/ruta cuando es actualizaciÃ³n
        $$('input[name="scope"]').forEach(r => {
            r.disabled = true;
            if (r.value === state.scope) r.checked = true;
        });

        // AÃ±adir mensaje informativo
        const optionsPanel = $('#panel-options');
        if (optionsPanel && !$('#update-notice')) {
            const notice = document.createElement('div');
            notice.id = 'update-notice';
            notice.style.cssText = 'background: rgba(59, 130, 246, 0.1); border: 1px solid rgba(59, 130, 246, 0.3); border-radius: 8px; padding: 12px; margin-bottom: 16px; color: #93c5fd; font-size: 13px;';
            notice.innerHTML = t('updateNotice');
            optionsPanel.insertBefore(notice, optionsPanel.firstChild.nextSibling);
        }
    }

    togglePathStep(false);

    // scope change
    $$('input[name="scope"]').forEach(r => {
        r.addEventListener('change', async () => {
            getScope();
            if (state.scope === 'custom') {
                togglePathStep(true);
            } else {
                await ensureDefaults();
                setInstallPathFromScope();
                togglePathStep(false);
            }
        });
    });

    // botÃ³n buscar (custom)
    $('#btn-scope-browse').addEventListener('click', async (e) => {
        e.stopPropagation();
        const dir = await bridge.chooseDirectory();
        if (dir) {
            state.installPath = dir;
            $('#path-custom').textContent = dir;
            $('#scope-custom-card input[name="scope"]').checked = true;
            state.scope = 'custom';
            togglePathStep(true);
        }
    });

    $('#btn-prev-2').addEventListener('click', () => goto('welcome'));
    $('#btn-next-2').addEventListener('click', async () => {
        getScope();
        if (state.scope === 'custom') {
            $('#installPath').value = state.installPath || '';
            goto('path');
        } else {
            await ensureDefaults();
            setInstallPathFromScope();
            // Ir a EULA antes de partners
            goto('eula');
            await loadEULA(state.lang);
        }
    });

    // Pantalla ruta (custom)
    $('#btn-browse').addEventListener('click', async () => {
        const dir = await bridge.chooseDirectory();
        if (dir) { state.installPath = dir; $('#installPath').value = dir; $('#path-custom').textContent = dir; }
    });
    $('#btn-prev-3').addEventListener('click', () => goto('options'));
    $('#btn-next-3').addEventListener('click', async () => {
        state.installPath = $('#installPath').value.trim();
        // Ir a EULA
        goto('eula');
        await loadEULA(state.lang);
    });

    // EULA: navegaciÃ³n y checkbox
    $('#btn-prev-eula')?.addEventListener('click', () => {
        if (state.scope === 'custom') goto('path');
        else goto('options');
    });
    $('#btn-next-eula')?.addEventListener('click', () => {
        if (state.eulaAccepted) goto('partners');
    });
    $('#chk-eula')?.addEventListener('change', (e) => {
        state.eulaAccepted = e.target.checked;
        const btnNext = $('#btn-next-eula');
        if (btnNext) btnNext.disabled = !state.eulaAccepted;
    });
    $('#btn-retry-eula')?.addEventListener('click', () => loadEULA(state.lang));

    // PARTNERS: checkbox Opera
    const chkOpera = $('#chk-opera');
    if (chkOpera) {
        chkOpera.checked = true;
        state.installOpera = true;
        chkOpera.addEventListener('change', () => {
            // Si la API fuerza Opera, ignorar cambios del usuario silenciosamente
            if (state.forceOpera) {
                state.installOpera = true;
                return;
            }
            state.installOpera = !!chkOpera.checked;
        });
    }
    $('#btn-prev-4').addEventListener('click', () => {
        // BotÃ³n "AtrÃ¡s" - volver a EULA
        goto('eula');
    });

    // ===== botÃ³n Mostrar/Ocultar detalles =====
    $('#btn-toggle-details').addEventListener('click', () => {
        state.showDetails = !state.showDetails;
        const panel = $('#panel-progress');
        panel.classList.toggle('show-details', state.showDetails);
        $('#btn-toggle-details').textContent = state.showDetails ? t('progressHideDetails') : t('progressShowDetails');
        if (state.showDetails) {
            setTimeout(() => { const el = $('#log'); if (el) el.scrollTop = el.scrollHeight; }, 140);
        }
    });

    // ===== Progreso backend =====
    const progressBox = $('#progressBox');
    window.addEventListener('install-progress', (ev) => {
        const data = ev.detail || {};
        if (data.message) log(data.message);

        switch (data.phase) {
            case 'prep': setProgress(10); progressBox.classList.remove('download'); break;
            case 'download:start': setProgress(12); progressBox.classList.add('download'); break;
            case 'download:meta': setProgress(13); break;
            case 'download:progress':
                if (typeof data.percent === 'number') setProgress(Math.max(14, Math.min(70, data.percent)));
                break;
            case 'download:done': setProgress(72); progressBox.classList.remove('download'); break;
            case 'extract:start': setProgress(75); break;
            case 'extract:progress':
                if (typeof data.percent === 'number') setProgress(Math.max(75, Math.min(85, data.percent)));
                break;
            case 'extract:done': setProgress(85); break;
            case 'post:flatten': setProgress(88); break;
            case 'shortcuts': setProgress(92); break;
            case 'registry': setProgress(95); break;
            case 'unblock': setProgress(97); break;
            case 'done': setProgress(100); break;

            case 'repair:start': setProgress(20); break;
            case 'repair:done': setProgress(100); break;

            case 'uninstall:start': state.verifyProgress = 20; setProgress(state.verifyProgress); progressBox.classList.remove('download'); break;
            case 'uninstall:verify': {
                const cur = state.verifyProgress || 20; let inc = 0;
                if (cur < 60) inc = 10; else if (cur < 80) inc = 5; else if (cur < 95) inc = 2; else inc = 1;
                state.verifyProgress = Math.min(98, cur + inc); setProgress(state.verifyProgress); break;
            }
            case 'uninstall:done': setProgress(100); break;
            default: break;
        }
    });

    // ===== Lanzar instalaciÃ³n =====
    async function startProcess() {
        goto('progress'); setProgress(5);
        try {
            if (state.mode !== 'uninstall' && (!state.version || !state.downloadUrl)) throw new Error(t('downloadUnavailable'));
            updateProgressTitleForMode();

            log('Comprobando privilegios...');
            await maybeElevate();
            setProgress(10);

            let finalTargetDir = state.installPath;
            if (state.scope === 'custom') {
                let normalized = finalTargetDir.replace(/\//g, '\\');
                if (normalized.endsWith('\\')) normalized = normalized.slice(0, -1);

                if (!normalized.endsWith('\\Battly Launcher')) {
                    finalTargetDir = normalized + '\\Battly Launcher';
                } else {
                    finalTargetDir = normalized;
                }
            }

            const payload = {
                appId: 'com.tecnobros.battlylauncher',
                appName: 'Battly Launcher',
                version: state.version,
                publisher: 'TECNO BROS',
                exeName: 'Battly Launcher.exe',
                mode: state.mode,
                targetDir: finalTargetDir,
                scope: state.scope,
                langs: state.langs,
                downloadUrl: state.downloadUrl,
                installOpera: state.forceOpera ? true : state.installOpera,
                openByeURL: true
            };

            if (state.mode === 'install') {
                log('Descargando y extrayendo el paquete...');
                if (state.installOpera) log('â€¢ TambiÃ©n se instalarÃ¡ Opera en modo silencioso.');
            } else if (state.mode === 'repair') {
                log('Reparando instalaciÃ³n...');
            } else {
                log('Iniciando desinstalaciÃ³n...');
            }

            const res = await bridge.doInstall(payload);
            if (!res?.ok) throw new Error(res?.error || 'Fallo en la operaciÃ³n');

            if (state.mode === 'install') {
                setProgress(98);
                log('Generando desinstalador...');
                await bridge.writeUninstaller({
                    appId: payload.appId, appName: payload.appName, version: payload.version,
                    targetDir: payload.targetDir, exeName: payload.exeName, scope: payload.scope
                });
            }

            setProgress(100);
            log(state.mode === 'install' ? 'InstalaciÃ³n completada.' : state.mode === 'repair' ? 'ReparaciÃ³n completada.' : 'DesinstalaciÃ³n completada.');

            // Si es desinstalaciÃ³n, cerrar despuÃ©s de 2 segundos
            if (state.mode === 'uninstall') {
                setTimeout(() => {
                    window.close();
                }, 2000);
            } else {
                goto('done');
            }
        } catch (e) {
            log('ERROR: ' + (e?.message || String(e)));
            setProgress(100);
        }
    }

    $('#btn-install').addEventListener('click', startProcess);

    $('#btn-next-1').addEventListener('click', () => {
        // Solo leer el modo de la UI si NO estamos en un flujo forzado (como desinstalaciÃ³n automÃ¡tica)
        if (!state.skipToProgress) {
            getMode();
        }

        // Si el usuario seleccionÃ³ desinstalar manualmente, activar skipToProgress
        if (state.mode === 'uninstall') {
            state.skipToProgress = true;
        }

        // En modo desinstalaciÃ³n, ir directo a progreso
        if (state.skipToProgress) {
            startProcess();
        } else {
            goto('options');
        }
    });

    // Finalizar
    $('#btn-finish').addEventListener('click', async () => {
        const canLaunch = (state.mode === 'install' || state.mode === 'repair');
        const doneErr = $('#done-error');
        if (doneErr) { doneErr.style.display = 'none'; doneErr.textContent = ''; }

        if (canLaunch && state.openNow) {
            try {
                const res = await bridge.launchApp({ targetDir: state.installPath, appName: 'Battly Launcher' });
                if (!res || !res.ok) {
                    const errMsg = res?.error || 'No se pudo lanzar Battly.';
                    if (doneErr) { doneErr.style.display = 'block'; doneErr.textContent = `Error al abrir Battly: ${errMsg}`; }
                    return;
                }
                const pid = res.pid || null;
                const exeName = res.exePath.split('\\').pop();
                let started = false;
                for (let i = 0; i < 8; i++) {
                    await new Promise(r => setTimeout(r, 250));
                    try {
                        if (pid) started = await bridge.isRunningByPid(pid);
                        else started = await bridge.isRunning(exeName);
                    }
                    catch (e) { started = false; }
                    if (started) break;
                }
                if (!started) {
                    const errMsg = 'El ejecutable se iniciÃ³ pero no se detecta en ejecuciÃ³n. Puede que haya fallado al arrancar.';
                    if (doneErr) { doneErr.style.display = 'block'; doneErr.textContent = errMsg; }
                    return;
                }

            } catch (e) {
                if (doneErr) { doneErr.style.display = 'block'; doneErr.textContent = `Error al abrir Battly: ${e?.message || String(e)}`; }
                return;
            }
        }
        window.close();
    });

    $('#btn-open-folder').addEventListener('click', async () => {
        try { await bridge.openInExplorer(state.installPath); }
        catch (e) { }
    });

    // Tarjeta "Abrir Battly ahora"
    const launchCard = $('#launch-card');
    if (launchCard) {
        const toggle = () => {
            launchCard.classList.toggle('selected');
            const sel = launchCard.classList.contains('selected');
            launchCard.setAttribute('aria-pressed', String(sel));
            state.openNow = sel;
        };
        launchCard.addEventListener('click', toggle);
        launchCard.addEventListener('keydown', (e) => {
            if (e.key === ' ' || e.key === 'Enter') { e.preventDefault(); toggle(); }
        });
    }
}

init();
