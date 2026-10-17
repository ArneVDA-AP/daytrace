import { LegacyFrame } from "@/app/LegacyFrame";
import { useAppState } from "@/app/state";
import { Onboarding } from "@/views/Onboarding";

/*
 * INTERIM (owned by the Onboarding engineer): the first-run setup replaces the whole shell. It
 * shows while settings.onboardingCompleted is false, or when Settings / the command menu asks for
 * it again (OPEN_ONBOARDING_EVENT); `onDone` returns to the shell.
 */
export default function OnboardingScreen({ firstRun, onDone }: { firstRun: boolean; onDone: () => void }) {
  const app = useAppState();
  return (
    <div className="flex h-full flex-col bg-background">
      <LegacyFrame>
        <Onboarding
          dto={app.settings}
          firstRun={firstRun}
          onDone={async () => {
            await app.reload();
            onDone();
          }}
        />
      </LegacyFrame>
    </div>
  );
}
