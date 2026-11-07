"""Small reproducible wall-time probe; not a memory or throughput certification."""
import json
import platform
import subprocess
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DIRECTORY = ROOT / "target" / "v1alpha1-benchmark"
DIRECTORY.mkdir(parents=True, exist_ok=True)
EXE = ROOT / "target" / "debug" / ("loj-validator.exe" if platform.system() == "Windows" else "loj-validator")
manifest = {
    "schemaVersion": "loj/v1alpha1",
    "law": {"id": "benchmark", "title": "架空ベンチマーク法", "type": "act", "number": None, "promulgatedAt": None, "language": "ja"},
    "documents": [{"path": "main.md", "parent": "main"}],
}
(DIRECTORY / "law.yaml").write_text(json.dumps(manifest, ensure_ascii=False), encoding="utf-8")
results = []
for count in (100, 1000, 5000):
    source = "".join(f"# 第{n}条\n<!-- loj:paragraph number=1 -->\n本文😀。\n<!-- loj:end-paragraph -->\n" for n in range(1, count + 1))
    (DIRECTORY / "main.md").write_bytes(source.encode("utf-8"))
    start = time.perf_counter()
    proc = subprocess.run([str(EXE), "check", str(DIRECTORY), "--format", "json"], capture_output=True, check=True)
    elapsed = time.perf_counter() - start
    report = json.loads(proc.stdout)
    assert report["status"] == "Passed"
    results.append({"articles": count, "inputBytes": len(source.encode("utf-8")), "jsonBytes": len(proc.stdout), "secondsIncludingProcessAndJson": round(elapsed, 4)})
output = {"platform": platform.platform(), "build": "debug", "runsPerSize": 1, "results": results}
(DIRECTORY / "results.json").write_text(json.dumps(output, indent=2) + "\n", encoding="utf-8")
print(json.dumps(output, indent=2))
