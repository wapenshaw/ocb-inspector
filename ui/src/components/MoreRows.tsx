// Long tables render in slices; this says how much is left out.

/** Rows a table renders at most (search or filters narrow the rest). */
export const ROW_LIMIT = 500;

export function MoreRows({ total, limit = ROW_LIMIT }: { total: number; limit?: number }) {
  if (total <= limit) return null;
  return (
    <p className="more quiet">
      Showing {limit} of {total}. Search to narrow the list.
    </p>
  );
}
