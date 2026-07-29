import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";

function gzipDirectory(dir) {
  const entries = fs.readdirSync(dir, { withFileTypes: true });

  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name);

    if (entry.isDirectory()) {
      gzipDirectory(fullPath);
    } else if (entry.isFile() && !entry.name.endsWith(".gz")) {
      const fileBuffer = fs.readFileSync(fullPath);
      const gzippedBuffer = zlib.gzipSync(fileBuffer, { level: 9 });

      fs.writeFileSync(`${fullPath}.gz`, gzippedBuffer);
      fs.unlinkSync(fullPath);
    }
  }
}

gzipDirectory("./dist");
console.log("gzip success!");
