import * as React from "react";
import { useAppSettings } from "@/app/state";
import { Switch } from "@/components/ui/switch";
import { FieldError, Group, Row } from "./fields";
import { useFieldSaves } from "./save";

/**
 * Settings > AI tools: "Show Claude Code session cards" (features/session_cards.rs). Off by
 * default, because it reads a folder outside Daytrace's own data: the finished cards that the
 * start-card plugin for Claude Code writes.
 */
export function SessionCardsGroup() {
  const s = useAppSettings().settings.settings;
  const fields = useFieldSaves();
  const id = React.useId();
  const folder = (s.sessionCardsDir ?? "").trim();
  return (
    <Group title="Claude Code session cards">
      <Row
        label="Show Claude Code session cards"
        htmlFor={`${id}-show`}
        description={
          <>
            <p>
              The start-card plugin for Claude Code keeps a card for each finished task: the outcome the session worked toward, its check, and the grade (pass,
              partial or fail). With this on, each card shows on the timeline card that covers the time the session finished, and connected AI tools get it with
              the timeline. Daytrace’s own chat does not.
            </p>
            <p>
              Daytrace only reads the cards, from{" "}
              {folder ? (
                <span className="font-mono text-caption [overflow-wrap:anywhere]" translate="no">
                  {folder}
                </span>
              ) : (
                <>
                  <span className="font-mono text-caption" translate="no">
                    start-card/finished
                  </span>{" "}
                  in your Claude config folder (
                  <span className="font-mono text-caption" translate="no">
                    .claude
                  </span>{" "}
                  in your user folder)
                </>
              )}
              . Nothing is copied or sent. Sessions that ran in a temporary folder are counted, not shown.
            </p>
          </>
        }
      >
        <Switch id={`${id}-show`} checked={fields.shown("sessionCards", s.showSessionCards ?? false)} onCheckedChange={(v) => void fields.apply("sessionCards", v, { showSessionCards: v })} />
        <FieldError>{fields.error("sessionCards")}</FieldError>
      </Row>
    </Group>
  );
}
