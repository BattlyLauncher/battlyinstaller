const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');

function harness(getLatestVersion) {
  const source = fs.readFileSync(path.join(__dirname, '../src/renderer.js'), 'utf8');
  const elements = new Map();
  const document = { querySelector(selector) {
    if (!elements.has(selector)) elements.set(selector, {
      hidden: true, style: {}, classList: { add() {}, remove() {} }, textContent: '',
    });
    return elements.get(selector);
  } };
  const context = vm.createContext({ document, bridge: { getLatestVersion } });
  vm.runInContext(source.slice(0, source.indexOf('async function init()')), context);
  const start = source.indexOf('    async function startProcess()');
  const end = source.indexOf("    $('#btn-install').addEventListener", start);
  vm.runInContext(source.slice(start, end), context);
  vm.runInContext(`
    t = key => key;
    goto = () => {};
    log = () => {};
    updateProgressTitleForMode = () => {};
    maybeElevate = async () => {};
    globalThis.api = { loadLatestVersion, startProcess, state };
  `, context);
  return { context, elements, api: context.api };
}

test('concurrent version requests await one lookup', async () => {
  let finish, calls = 0;
  const deferred = new Promise(resolve => { finish = resolve; });
  const { api } = harness(() => { calls++; return deferred; });
  const first = api.loadLatestVersion();
  const second = api.loadLatestVersion();
  assert.equal(calls, 1);
  finish({ ok: true, version: '3.1.0', downloadUrl: 'https://github.com/download.zip' });
  await Promise.all([first, second]);
  assert.equal(api.state.version, '3.1.0');
});

test('a failed lookup shows its cause and a retry can finish installation', async () => {
  let calls = 0, installations = 0;
  const { context, elements, api } = harness(async () => {
    if (++calls === 1) throw Error('GitHub HTTP 403');
    return { ok: true, version: '3.1.0', downloadUrl: 'https://github.com/download.zip' };
  });
  vm.runInContext(`
    globalThis.messages = [];
    log = message => messages.push(message);
    bridge.doInstall = async () => { globalThis.installed(); return { ok: true }; };
    bridge.writeUninstaller = async () => {};
  `, context);
  context.installed = () => installations++;
  await api.startProcess();
  assert.equal(installations, 0);
  assert.equal(elements.get('#btn-retry-install').hidden, false);
  assert(context.messages.some(message => message.includes('403')));
  assert.equal(elements.get('#bar').style.width, '0%');
  await api.startProcess();
  assert.equal(installations, 1);
  assert.equal(calls, 2);
  assert.equal(elements.get('#btn-retry-install').hidden, true);
});
