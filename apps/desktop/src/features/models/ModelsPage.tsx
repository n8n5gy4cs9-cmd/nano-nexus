import { useMutation, useQueryClient } from "@tanstack/react-query";
import { FolderPlus, ShieldCheck, Upload } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import type { InstalledModel } from "@/bindings";
import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
  EmptyState,
  Skeleton,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui";
import { api } from "@/lib/api";
import { toIpcException } from "@/lib/ipc";

import { CatalogCardView, DownloadStrip, InstalledCard } from "./ModelCards";
import { DuoPanel } from "./DuoPanel";
import { ModelDrawer } from "./ModelDrawer";
import { RuntimePanel } from "./RuntimePanel";
import { SearchTab } from "./SearchTab";
import { useModels } from "./useModels";

const fail = (e: unknown) => void toast.error(toIpcException(e).message);

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <h2 className="text-small font-semibold tracking-wide text-muted uppercase">{title}</h2>
      {children}
    </section>
  );
}

const GRID = "grid gap-3 sm:grid-cols-2 xl:grid-cols-3";

/** Models page (UI-DESIGN §6.4): runtime panel, Installed, Discover, Search. */
export function ModelsPage() {
  const qc = useQueryClient();
  const { installed, catalog, downloads } = useModels();
  const [detail, setDetail] = useState<InstalledModel | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<InstalledModel | null>(null);
  const [tab, setTab] = useState("installed");
  const refresh = () => void qc.invalidateQueries({ queryKey: ["models"] });

  const activate = useMutation({
    mutationFn: (id: string) => api.models.activate(id),
    onSuccess: (r) => {
      if (r.warning) toast.warning(r.warning);
      else toast.success("Switching model. The runtime is restarting.");
      refresh();
    },
    onError: (e) => {
      fail(e);
      refresh();
    },
  });
  const remove = useMutation({
    mutationFn: (id: string) => api.models.delete(id),
    onSuccess: () => {
      setConfirmDelete(null);
      toast.success("Removed.");
      refresh();
    },
    onError: (e) => {
      setConfirmDelete(null);
      fail(e);
    },
  });
  const downloadCatalog = useMutation({ mutationFn: (id: string) => api.models.downloadCatalog(id), onSuccess: refresh, onError: fail });
  const control = useMutation({
    mutationFn: ({ id, op }: { id: string; op: "pause" | "resume" | "cancel" }) => api.models[op](id),
    onSuccess: refresh,
    onError: fail,
  });
  const importFile = useMutation({
    mutationFn: async () => {
      const path = await api.models.pickFile();
      return path ? api.models.import(path, true) : null;
    },
    onSuccess: (m) => {
      if (m) toast.success(`Added “${m.name}”.`);
      refresh();
    },
    onError: fail,
  });
  const addFolder = useMutation({
    mutationFn: async () => {
      const dir = await api.fs.pickFolder();
      if (!dir) return null;
      const settings = await api.settings.get();
      const dirs = [...new Set([...(settings["models.extra_dirs"] ?? []), dir])];
      await api.settings.set("models.extra_dirs", dirs);
      return api.models.rescan();
    },
    onSuccess: (n) => {
      if (n !== null) toast.success(`Found ${n} model${n === 1 ? "" : "s"} in your folders.`);
      refresh();
    },
    onError: fail,
  });
  const verify = useMutation({
    mutationFn: api.models.verifyBundled,
    onSuccess: (rs) => {
      const bad = rs.filter((r) => !r.ok);
      if (bad.length === 0) toast.success(`All ${rs.length} bundled model${rs.length === 1 ? "" : "s"} verified.`);
      else toast.error(`${bad.length} bundled model${bad.length === 1 ? " is" : "s are"} damaged or missing: ${bad.map((b) => b.id).join(", ")}`);
    },
    onError: fail,
  });

  const activeId = installed.data?.find((m) => m.active)?.id ?? null;
  const active = (downloads.data ?? []).filter((d) => ["queued", "running", "paused", "failed"].includes(d.status));
  const bundledGroups = [
    { title: "Bundled", test: (m: InstalledModel) => m.source === "builtin" },
    { title: "Downloaded", test: (m: InstalledModel) => m.source === "download" },
    { title: "Imported and found in folders", test: (m: InstalledModel) => m.source === "import" || m.source === "discovered" },
  ];
  const ctl = (id: string, op: "pause" | "resume" | "cancel") => control.mutate({ id, op });

  return (
    <div className="mx-auto flex max-w-6xl flex-col gap-4 p-6">
      <header>
        <h1 className="text-lead font-semibold">Models</h1>
        <p className="text-body text-muted">Download, switch, and manage the models that run on your computer.</p>
      </header>
      <RuntimePanel />
      <DuoPanel models={installed.data ?? []} />
      <Tabs value={tab} onValueChange={setTab}>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <TabsList>
            <TabsTrigger value="installed">Installed</TabsTrigger>
            <TabsTrigger value="discover">Discover</TabsTrigger>
            <TabsTrigger value="search">Search</TabsTrigger>
          </TabsList>
          <div className="flex flex-wrap gap-2">
            <Button size="sm" disabled={verify.isPending} onClick={() => verify.mutate()}>
              <ShieldCheck /> Verify bundled
            </Button>
            <Button size="sm" disabled={addFolder.isPending} onClick={() => addFolder.mutate()}>
              <FolderPlus /> Add folder
            </Button>
            <Button size="sm" disabled={importFile.isPending} onClick={() => importFile.mutate()}>
              <Upload /> Add a .gguf file
            </Button>
          </div>
        </div>

        <TabsContent value="installed" className="mt-4 flex flex-col gap-5">
          {active.length > 0 ? (
            <Section title="Downloading">
              <div className="flex flex-col gap-3">
                {active.map((d) => (
                  <div key={d.id} className="rounded-card border border-border bg-surface p-3 shadow-1">
                    <p className="mb-1.5 truncate text-body font-medium">{d.file}</p>
                    <DownloadStrip d={d} onPause={() => ctl(d.id, "pause")} onResume={() => ctl(d.id, "resume")} onCancel={() => ctl(d.id, "cancel")} />
                  </div>
                ))}
              </div>
            </Section>
          ) : null}
          {installed.isLoading ? (
            <div className={GRID}>
              {Array.from({ length: 3 }, (_, i) => (
                <Skeleton key={i} className="h-40" />
              ))}
            </div>
          ) : (installed.data ?? []).length === 0 ? (
            <EmptyState title="No models yet" description="Download one from Discover, or add a .gguf file you already have." action={<Button variant="primary" onClick={() => setTab("discover")}>Browse models</Button>} />
          ) : (
            bundledGroups.map((g) => {
              const list = (installed.data ?? []).filter(g.test);
              return list.length === 0 ? null : (
                <Section key={g.title} title={g.title}>
                  <div className={GRID}>
                    {list.map((m) => (
                      <InstalledCard key={m.id} m={m} busy={activate.isPending} onUse={() => activate.mutate(m.id)} onDelete={() => setConfirmDelete(m)} onDetails={() => setDetail(m)} />
                    ))}
                  </div>
                </Section>
              );
            })
          )}
        </TabsContent>

        <TabsContent value="discover" className="mt-4">
          {catalog.isLoading ? (
            <div className={GRID}>
              {Array.from({ length: 6 }, (_, i) => (
                <Skeleton key={i} className="h-48" />
              ))}
            </div>
          ) : (
            <div className={GRID}>
              {(catalog.data ?? []).map((c) => (
                <CatalogCardView
                  key={c.entry.id}
                  c={c}
                  activeId={activeId}
                  busy={activate.isPending}
                  onDownload={() => downloadCatalog.mutate(c.entry.id)}
                  onUse={() => c.installed_id && activate.mutate(c.installed_id)}
                  onPause={() => c.download && ctl(c.download.id, "pause")}
                  onResume={() => c.download && ctl(c.download.id, "resume")}
                  onCancel={() => c.download && ctl(c.download.id, "cancel")}
                />
              ))}
            </div>
          )}
        </TabsContent>

        <TabsContent value="search" className="mt-4">
          <SearchTab
            onStarted={() => {
              refresh();
              setTab("installed");
            }}
          />
        </TabsContent>
      </Tabs>

      {detail ? <ModelDrawer m={detail} onClose={() => setDetail(null)} /> : null}
      {confirmDelete ? (
        <Dialog open onOpenChange={(o) => !o && setConfirmDelete(null)}>
          <DialogContent aria-describedby="del-desc">
            <DialogTitle>{confirmDelete.removal === "unlink" ? "Remove from the list?" : "Delete this model?"}</DialogTitle>
            <DialogDescription id="del-desc">
              {confirmDelete.removal === "unlink"
                ? `“${confirmDelete.name}” stays where it is; NanoNexus just stops listing it.`
                : `“${confirmDelete.name}” will be deleted from this computer. You can download it again later.`}
            </DialogDescription>
            <DialogFooter>
              <Button onClick={() => setConfirmDelete(null)}>Cancel</Button>
              <Button variant="danger" disabled={remove.isPending} onClick={() => remove.mutate(confirmDelete.id)}>
                {confirmDelete.removal === "unlink" ? "Remove" : "Delete"}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      ) : null}
    </div>
  );
}
