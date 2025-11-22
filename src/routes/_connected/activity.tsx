/**
 * Activity Monitor Route
 *
 * Route for the activity monitoring interface that shows query execution logs,
 * statistics, and performance metrics.
 */

import { createFileRoute } from '@tanstack/react-router';
import { ActivityMonitor } from '@/components/ActivityMonitor';

export const Route = createFileRoute('/_connected/activity')({
  component: ActivityMonitorPage,
  validateSearch: (search: Record<string, unknown>) => {
    return search;
  },
});

function ActivityMonitorPage() {
  return (
    <div className="h-full">
      <ActivityMonitor />
    </div>
  );
}
