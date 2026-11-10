export type RemoteFile = {
  path: string;
  size: number;
  extension: string;
  attributes: { kind: number; value: number }[];
};

export type RemoteDirectory = { path: string; files: RemoteFile[] };

export type RemoteFolder = {
  path: string;
  remotePath: string;
  name: string;
  parent: string;
  children: string[];
  files: RemoteFile[];
};

export const normalizeRemotePath = (path: string) => path.split(/[\\/]/).filter(Boolean).join("\\");

// Catalogues often omit empty parent entries. Materialize those ancestors so
// every folder can be reached from the root and navigation follows segments.
export function buildRemoteTree(directories: RemoteDirectory[]): Map<string, RemoteFolder> {
  const tree = new Map<string, RemoteFolder>();
  tree.set("", { path: "", remotePath: "", name: "", parent: "", children: [], files: [] });
  for (const directory of directories) {
    const parts = normalizeRemotePath(directory.path).split("\\").filter(Boolean);
    let parent = "";
    for (const part of parts) {
      const path = parent ? `${parent}\\${part}` : part;
      if (!tree.has(path)) {
        tree.set(path, { path, remotePath: path, name: part, parent, children: [], files: [] });
        tree.get(parent)!.children.push(path);
      }
      parent = path;
    }
    const node = tree.get(parent)!;
    node.remotePath = directory.path;
    node.files = [...new Map([...node.files, ...directory.files].map((file) => [file.path, file])).values()];
  }
  for (const node of tree.values()) {
    node.children.sort((a, b) => tree.get(a)!.name.localeCompare(tree.get(b)!.name, undefined, { numeric: true }));
  }
  return tree;
}
