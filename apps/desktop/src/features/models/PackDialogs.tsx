import { useQueryClient } from "@tanstack/react-query";
import { useRef, useState } from "react";
import { toast } from "sonner";

import type { InstalledModel, PackExport, PackImport, PackProgress } from "@/bindings";
import { Button, Dialog, DialogContent, DialogDescription, DialogFooter, DialogTitle, Input, Progress } from "@/components/ui";
import { api } from "@/lib/api";
import { formatBytes } from "@/lib/format";
import { toIpcException } from "@/lib/ipc";

/** One running export or import: the progress frame, a cancel handle, and `run` to start it. */
function usePackJob<T>() {
  const [progress, setProgress] = useState<PackProgress | null>(null);
  const [busy, setBusy] = useState(false);
  const job = useRef<string | null>(null);
  const run = async (work: (jobId: string, onProgress: (p: PackProgress) => void) => Promise<T>): Promise<T | null> => {
    const id = crypto.randomUUID();
    job.current = id;
    setBusy(true);
    setProgress(null);
    try {
      return await work(id, setProgress);
    } catch (e) {
      const err = toIpcException(e);
      if (err.code !== "cancelled") toast.error(err.message);
      return null;
    } finally {
      job.current = null;
      setBusy(false);
      setProgress(null);
    }
  };
  const cancel = () => {
    if (job.current) void api.models.packCancel(job.current);
  };
  return { progress, busy, run, cancel };
}

function JobProgress({ p, verb }: { p: PackProgress | null; verb: string }) {
  const value = p && p.bytes_total > 0 ? p.bytes_done / p.bytes_total : undefined;
  return (
    <div className="flex flex-col gap-1.5" aria-live="polite">
      <Progress value={value} label={verb} />
      <p className="text-small text-muted">
        {p ? `${p.phase === "verifying" ? "Checking" : verb} ${p.model_id} (${p.model_index} of ${p.model_total}) · ${formatBytes(p.bytes_done)} of ${formatBytes(p.bytes_total)}` : "Starting…"}
      </p>
    </div>
  );
}

/** Models → "Export model pack…": pick models, pick where to write them (a USB stick), copy and verify. */
export function ExportPackDialog({ models, onClose }: { models: InstalledModel[]; onClose: () => void }) {
  const { progress, busy, run, cancel } = usePackJob<PackExport>();
  const [picked, setPicked] = useState<Set<string>>(() => new Set(models.filter((m) => m.source !== "discovered").map((m) => m.id)));
  const [name, setName] = useState("models");
  const [done, setDone] = useState<PackExport | null>(null);
  const toggle = (id: string) =>
    setPicked((cur) => {
      const next = new Set(cur);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  const total = models.filter((m) => picked.has(m.id)).reduce((n, m) => n + m.bytes, 0);

  const start = async () => {
    const dest = await api.models.pickFolder();
    if (!dest) return;
    const res = await run((jobId, onProgress) => api.models.packExport(jobId, [...picked], dest, name.trim() || null, onProgress));
    if (res) setDone(res);
  };

  return (
    <Dialog open onOpenChange={(o) => !o && !busy && onClose()}>
      <DialogContent aria-describedby="pack-export-desc" className="w-[min(560px,calc(100vw-32px))]">
        <DialogTitle>Export a model pack</DialogTitle>
        <DialogDescription id="pack-export-desc">
          Copy models onto a USB stick or folder. On any other computer, choose “Import model pack…” to add them, with no internet needed.
        </DialogDescription>
        {done ? (
          <div className="flex flex-col gap-1.5">
            <p className="text-body font-medium">
              Done: {done.models} model{done.models === 1 ? "" : "s"}, {formatBytes(done.bytes)}
            </p>
            <p className="text-small break-all text-muted">{done.dir}</p>
          </div>
        ) : busy ? (
          <JobProgress p={progress} verb="Copying" />
        ) : (
          <>
            <ul className="flex max-h-56 flex-col gap-1 overflow-y-auto" aria-label="Models to include">
              {models.map((m) => (
                <li key={m.id}>
                  <label className="flex items-center gap-2 text-body">
                    <input type="checkbox" checked={picked.has(m.id)} onChange={() => toggle(m.id)} />
                    <span className="min-w-0 flex-1 truncate">{m.name}</span>
                    <span className="text-small text-muted">{formatBytes(m.bytes)}</span>
                  </label>
                </li>
              ))}
            </ul>
            <label className="flex flex-col gap-1 text-small text-muted">
              Pack name
              <Input value={name} onChange={(e) => setName(e.target.value)} aria-label="Pack name" />
            </label>
          </>
        )}
        <DialogFooter>
          {busy ? (
            <Button onClick={cancel}>Stop</Button>
          ) : done ? (
            <Button variant="primary" onClick={onClose}>
              Close
            </Button>
          ) : (
            <>
              <Button onClick={onClose}>Cancel</Button>
              <Button variant="primary" disabled={picked.size === 0} onClick={() => void start()}>
                Choose folder and export ({formatBytes(total)})
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Models → "Import model pack…": pick the pack folder, verify every file, install. */
export function ImportPackDialog({ onClose }: { onClose: () => void }) {
  const qc = useQueryClient();
  const { progress, busy, run, cancel } = usePackJob<PackImport>();
  const [copy, setCopy] = useState(true);
  const [report, setReport] = useState<PackImport | null>(null);

  const start = async () => {
    const dir = await api.models.pickFolder();
    if (!dir) return;
    const res = await run((jobId, onProgress) => api.models.packImport(jobId, dir, copy, onProgress));
    void qc.invalidateQueries({ queryKey: ["models"] });
    if (res) setReport(res);
  };

  return (
    <Dialog open onOpenChange={(o) => !o && !busy && onClose()}>
      <DialogContent aria-describedby="pack-import-desc" className="w-[min(560px,calc(100vw-32px))]">
        <DialogTitle>Import a model pack</DialogTitle>
        <DialogDescription id="pack-import-desc">
          Choose the pack folder (it contains pack.json). Every file is checked before it is added, so a damaged copy is never used.
        </DialogDescription>
        {report ? (
          <div className="flex flex-col gap-1.5" aria-live="polite">
            <p className="text-body font-medium">
              {report.imported.length} imported{report.already_installed.length ? `, ${report.already_installed.length} already installed` : ""}
              {report.failed.length ? `, ${report.failed.length} failed` : ""}
            </p>
            {report.failed.map((f) => (
              <p key={f.id} className="text-small text-danger">
                {f.id}: {f.reason}
              </p>
            ))}
          </div>
        ) : busy ? (
          <JobProgress p={progress} verb="Copying" />
        ) : (
          <label className="flex items-start gap-2 text-body">
            <input type="checkbox" className="mt-1" checked={copy} onChange={(e) => setCopy(e.target.checked)} />
            <span>
              Copy the models into NanoNexus
              <span className="block text-small text-muted">Turn this off to use the files where they are. The drive then has to stay connected.</span>
            </span>
          </label>
        )}
        <DialogFooter>
          {busy ? (
            <Button onClick={cancel}>Stop</Button>
          ) : report ? (
            <Button variant="primary" onClick={onClose}>
              Close
            </Button>
          ) : (
            <>
              <Button onClick={onClose}>Cancel</Button>
              <Button variant="primary" onClick={() => void start()}>
                Choose pack folder
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
