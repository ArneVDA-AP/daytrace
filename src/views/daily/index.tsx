import { LegacyFrame } from "@/app/LegacyFrame";
import { useViewProps } from "@/app/view-props";
import { DayNav } from "@/components/day-nav";
import { PageHeader, TitleBadge } from "@/components/page-header";
import { formatDayTitle } from "@/lib/time";
import { DailyView } from "@/views/DailyView";

/*
 * INTERIM (owned by the Daily engineer): the old Daily view inside the new shell. The page header
 * with day navigation is final; replace <LegacyFrame> and its content with the ported view.
 */
export default function DailySection() {
  const props = useViewProps();
  return (
    <>
      <PageHeader
        label="Daily"
        title={formatDayTitle(props.day)}
        badge={props.day === props.today ? <TitleBadge>Today</TitleBadge> : undefined}
        nav={<DayNav />}
      />
      <LegacyFrame>
        <DailyView {...props} />
      </LegacyFrame>
    </>
  );
}
