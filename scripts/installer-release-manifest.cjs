const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');

function manifest(directory) {
  const installers = [
    ['Battly-Launcher-Windows.exe', 'x64', 0x8664],
    ['Battly-Launcher-win-ia32.exe', 'ia32', 0x014c],
  ].map(([file, arch, machine]) => {
    const bytes = fs.readFileSync(path.join(directory, file));
    const pe = bytes.readUInt32LE(60);
    const protocol = JSON.parse(fs.readFileSync(path.join(directory, `protocol-${arch}.json`), 'utf8'));
    if (bytes.toString('ascii', 0, 2) !== 'MZ' || bytes.toString('ascii', pe, pe + 4) !== 'PE\0\0' ||
      bytes.readUInt16LE(pe + 4) !== machine || protocol.schema !== 1 || protocol.protocol !== 2 ||
      protocol.qa !== false || protocol.arch !== arch) throw Error('Wrong installer architecture or update protocol: ' + file);
    return { file, arch, size: bytes.length, sha512: crypto.createHash('sha512').update(bytes).digest('base64') };
  });
  const result = { schema: 1, protocol: 2, qa: false, installers };
  fs.writeFileSync(path.join(directory, 'installer-protocol.json'), JSON.stringify(result, null, 2) + '\n');
  return result;
}
module.exports = { manifest };
if (require.main === module) manifest(path.resolve(process.argv[2]));
