import * as React from "react";
import { FolderOpen } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { useAppState } from "@/app/state";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { errorText } from "@/lib/utils";
import { FieldError, Group, Row } from "./fields";
import { useCommitField, useSave } from "./save";

/** A full path: a drive (C:\), a network share (\\server) or, outside Windows, a root slash. */
export function isAbsolutePath(p: string): boolean {
  return /^[A-Za-z]:[\\/]/.test(p) || /^[\\/]{2}[^\\/]/.test(p) || p.startsWith("/");
}

/** Settings > Daily recap: automatic writing, its time, and the Markdown export folder. */
export function RecapSection() {
  const app = useAppState();
  const s = app.settings.settings;
  const save = useSave();
  const id = React.useId();
  const [error, setError] = React.useState<{ field: string; text: string } | null>(null);
  const apply = (field: string, patch: Parameters<typeof save>[0]) => {
    setError(null);
    save(patch).catch((e) => setError({ field, text: `Couldn’t save: ${errorText(e)}` }));
  };
  const err = (field: string) => (error?.field === field ? error.text : null);

  const time = useCommitField(s.recapTime, (v) => save({ recapTime: v || "08:00" }));
  const folder = useCommitField(s.recapExportDir, (v) => save({ recapExportDir: v.trim() }), {
    validate: (v) => (v.trim() && !isAbsolutePath(v.trim()) ? "Use a full path for the recap folder, such as C:\\Users\\you\\OneDrive, or select Choose folder." : null),
  });

  async function chooseFolder() {
    setError(null);
    try {
      const dir = await open({ directory: true, multiple: false, defaultPath: s.recapExportDir || undefined });
      if (typeof dir === "string") {
        folder.revert();
        await save({ recapExportDir: dir });
      }
    } catch (e) {
      setError({ field: "folder", text: `Couldn’t open the folder picker: ${errorText(e)}` });
    }
  }

  return (
    <>
      <Group
        title="Standup recap"
        description="Each morning Daytrace can write a standup from your last day with at least 3 hours of activity: the highlights, the next task, and room for blockers. It uses the model under Model provider and costs one request a day. Generate in the Daily view writes one by hand."
      >
        <Row label="Write the recap automatically every day" htmlFor={`${id}-auto`} description="It does not run while recording is paused.">
          <Switch id={`${id}-auto`} checked={s.recapAuto} onCheckedChange={(v) => apply("auto", { recapAuto: v })} />
          <FieldError>{err("auto")}</FieldError>
        </Row>
        <Row label="Write it after" htmlFor={`${id}-time`} description={s.recapAuto ? "Daytrace writes it once a day, after this time." : "Turn on the automatic recap to set a time."}>
          <Input id={`${id}-time`} type="time" {...time.bind} disabled={!s.recapAuto} className="w-32 tnum" />
          <FieldError>{time.error}</FieldError>
        </Row>
      </Group>

      <Group
        title="Markdown export"
        description="Each day gets one file, such as Daytrace\2026-10-16.md, with the recap, the day’s numbers and its cards. Pick a folder your phone syncs, such as one in OneDrive."
      >
        <Row
          label="Save recaps as Markdown in"
          htmlFor={`${id}-folder`}
          stack
          description={
            s.recapAuto
              ? "With automatic recaps on, the file is written once a day after the time above. Export recap in the Daily view writes one by hand."
              : "Export recap in the Daily view writes the file. With automatic recaps on, it is also written once a day."
          }
        >
          <div className="flex w-full flex-wrap gap-2">
            <Input
              id={`${id}-folder`}
              {...folder.bind}
              placeholder="Not saved to a folder"
              spellCheck={false}
              autoComplete="off"
              className="min-w-0 flex-1 basis-72 font-mono text-meta placeholder:font-sans"
            />
            <Button type="button" variant="outline" onClick={() => void chooseFolder()}>
              <FolderOpen aria-hidden /> Choose folder…
            </Button>
            {s.recapExportDir && (
              <Button
                type="button"
                variant="ghost"
                onClick={() => {
                  folder.revert();
                  apply("folder", { recapExportDir: "" });
                }}
              >
                Stop saving
              </Button>
            )}
          </div>
          <FieldError>{folder.error ?? err("folder")}</FieldError>
        </Row>
      </Group>
    </>
  );
}
