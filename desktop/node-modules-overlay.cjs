/**
 * Updater stage may hold a node_modules *symlink* into the live install
 * so a source rebuild can reuse packages. That symlink must never be
 * copied over the real directory in DEST.new (GNU cp: "cannot overwrite
 * directory with non-directory"). After a swap it would also point at
 * itself.
 */
const fs = require("node:fs");
const path = require("node:path");

function nodeModulesOverlayPlan({ stageIsSymlink, stageIsDir }) {
  if (stageIsSymlink) return "skip";
  if (stageIsDir) return "replace";
  return "keep";
}

function detachLinkedNodeModules(root) {
  const p = path.join(root, "node_modules");
  try {
    const st = fs.lstatSync(p);
    if (st.isSymbolicLink()) {
      fs.unlinkSync(p);
      return { ok: true, detached: true, path: p };
    }
  } catch {
    /* missing */
  }
  return { ok: true, detached: false, path: p };
}

module.exports = {
  nodeModulesOverlayPlan,
  detachLinkedNodeModules,
};
