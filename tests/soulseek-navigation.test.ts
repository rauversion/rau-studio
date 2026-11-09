import assert from "node:assert/strict";
import test from "node:test";
import { buildRemoteTree, normalizeRemotePath } from "../src/components/sharing/remoteCatalogue.ts";

const file = (path: string) => ({ path, size: 2048, extension: "flac", attributes: [{ kind: 0, value: 900 }] });

test("omitted ancestors remain navigable and similarly named albums stay separate", () => {
  const tree = buildRemoteTree([
    { path: "Music\\Artist\\Album\\Disc 2", files: [file("track.flac")] },
    { path: "Music\\Artist\\Album 2", files: [file("other.flac")] }
  ]);
  assert.deepEqual(tree.get("")!.children, ["Music"]);
  assert.deepEqual(tree.get("Music\\Artist\\Album")!.children, ["Music\\Artist\\Album\\Disc 2"]);
  assert.equal(tree.get("Music\\Artist\\Album\\Disc 2")!.parent, "Music\\Artist\\Album");
  assert.equal(tree.get("Music")!.parent, "");
  assert.deepEqual(tree.get("Music\\Artist\\Album 2")!.files.map((f) => f.path), ["other.flac"]);
});

test("navigation normalizes separators while retaining the peer's directory and file names", () => {
  const track = file("01 canción.flac");
  const tree = buildRemoteTree([{ path: "Music/Artist/Album", files: [track] }]);
  const node = tree.get(normalizeRemotePath("Music/Artist/Album/"))!;
  assert.equal(node.remotePath, "Music/Artist/Album");
  assert.deepEqual(node.files, [track]);
  assert.equal(node.parent, "Music\\Artist");
});

test("empty folders remain visible and repeated directory entries do not duplicate files", () => {
  const tree = buildRemoteTree([
    { path: "Music\\Empty", files: [] },
    { path: "Music\\Album", files: [file("a.flac")] },
    { path: "Music/Album", files: [file("a.flac"), file("b.flac")] }
  ]);
  assert.deepEqual(tree.get("Music")!.children, ["Music\\Album", "Music\\Empty"]);
  assert.deepEqual(tree.get("Music\\Empty")!.files, []);
  assert.deepEqual(tree.get("Music\\Album")!.files.map((f) => f.path), ["a.flac", "b.flac"]);
});
