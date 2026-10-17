import * as React from "react";
import { FolderOpen } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { providerLabel } from "@/app/status";
import { useAppState } from "@/app/state";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { errorText } from "@/lib/utils";
import { FieldError, Group, Row } from "./fields";
import { useCommitField, useFieldSaves, useSave } from "./save";

/** A full path: a drive (C:\), a network share (\\server) or, outside Windows, a root slash. */
export function isAbsolutePath(p: string): boolean {
  return /^[A-Za-z]:[\\/]/.test(p) || /^[\\/]{2}[^\\/]/.test(p) || p.startsWith("/");
}

/** Settings > Daily recap: automatic writing, its time, its model, and the Markdown export folder. */
export function RecapSection() {
  const app = useAppState();
  const s = app.settings.settings;
  const save = useSave();
  const fields = useFieldSaves();
  const id = React.useId();
  const [folderError, setFolderError] = React.useState<string | null>(null);

  const time = useCommitField(s.recapTime, (v) => save({ recapTime: v || "08:00" }));
  const model = useCommitField(s.recapModel, (v) => save({ recapModel: v.trim() }));
  const folder = useCommitField(s.recapExportDir, (v) => save({ recapExportDir: v.trim() }), {
    validate: (v) => (v.trim() && !isAbsolutePath(v.trim()) ? "Use a full path for the recap folder, such as C:\\Users\\you\\OneDrive, or select Choose folder." : null),
  });

  async function chooseFolder() {
    setFolderError(null);
    try {
      const dir = await open({ directory: true, multiple: false, defaultPath: s.recapExportDir || undefined });
      if (typeof dir === "string") {
        folder.revert();
        await save({ recapExportDir: dir });
      }
    } catch (e) {
      setFolderError(`Couldn’t open the folder picker: ${errorText(e)}`);
    }
  }

  return (
    <>
      <Group
        title="Standup recap"
        description="Each morning Daytrace can write a standup from your last day with at least 3 hours of activity: the highlights, the next task, and room for blockers. It costs one request a day. Generate in the Daily view writes one by hand."
      >
        <Row label="Write the recap automatically every day" htmlFor={`${id}-auto`} description="It does not run while recording is paused.">
          <Switch id={`${id}-auto`} checked={fields.shown("auto", s.recapAuto)} onCheckedChange={(v) => void fields.apply("auto", v, { recapAuto: v })} />
          <FieldError>{fields.error("auto")}</FieldError>
        </Row>
        <Row label="Write it after" htmlFor={`${id}-time`} description={s.recapAuto ? "Daytrace writes it once a day, after this time." : "Turn on the automatic recap to set a time."}>
          <Input id={`${id}-time`} type="time" {...time.bind} disabled={!s.recapAuto} className="w-32 tnum" />
          <FieldError>{time.error}</FieldError>
        </Row>
        <Row
          label="Model for the recap"
          htmlFor={`${id}-model`}
          stack
          description={
            <p id={`${id}-model-help`}>
              On the same provider as the timeline, {providerLabel(app.settings)}. Leave it empty to use the timeline model,{" "}
              <span className="font-mono" translate="no">
                {s.model || "not set"}
              </span>
              . A larger model writes a better standup and costs more; it is one request a day.
            </p>
          }
        >
          <Input
            id={`${id}-model`}
            {...model.bind}
            placeholder={s.model ? `Same as the timeline (${s.model})` : "Same as the timeline"}
            spellCheck={false}
            autoComplete="off"
            aria-describedby={`${id}-model-help`}
            className="w-full max-w-md font-mono text-meta placeholder:font-sans"
          />
          <FieldError>{model.error}</FieldError>
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
              ? "With automatic recaps on, the file is written once a day after the time above. Export as Markdown in the Daily view writes one by hand."
              : "Export as Markdown in the Daily view writes the file. With automatic recaps on, it is also written once a day."
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
                  setFolderError(null);
                  void fields.apply("folder", "", { recapExportDir: "" });
                }}
              >
                Stop saving
              </Button>
            )}
          </div>
          <FieldError>{folder.error ?? folderError ?? fields.error("folder")}</FieldError>
        </Row>
      </Group>
    </>
  );
}
