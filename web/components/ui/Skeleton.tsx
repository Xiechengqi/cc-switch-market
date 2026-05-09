type SkeletonProps = { className?: string };

export function Skeleton({ className = "h-6 w-24" }: SkeletonProps) {
  return <span className={`inline-block animate-pulse rounded-md bg-slate-200 ${className}`} />;
}
