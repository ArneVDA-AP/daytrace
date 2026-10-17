import { LegacyFrame } from "@/app/LegacyFrame";
import { useViewProps } from "@/app/view-props";
import { DayNav } from "@/components/day-nav";
import { PageHeader, TitleBadge } from "@/components/page-header";
import { formatDayTitle } from "@/lib/time";
import { JournalView } from "@/views/JournalView";

/*
 * INTERIM (owned by the Goals engineer): the old Goals view (view id "journal": Dayflow's day
 * goals) inside the new shell. The header with day navigation is final.
 */
export default function GoalsSection() {
  const props = useViewProps();
  return (
    <>
      <PageHeader
        label="Goals"
        title={formatDayTitle(props.day)}
        badge={props.day === props.today ? <TitleBadge>Today</TitleBadge> : undefined}
        nav={<DayNav />}
      />
      <LegacyFrame>
        <JournalView {...props} />
      </LegacyFrame>
    </>
  );
}
