import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/lib/api";
import type { Category } from "@/lib/types";

const KEY = ["categories"];

export function useCategories() {
  return useQuery<Category[]>({ queryKey: KEY, queryFn: () => api.get<Category[]>("/categories") });
}

function useInvalidate() {
  const qc = useQueryClient();
  return () => {
    qc.invalidateQueries({ queryKey: KEY });
    qc.invalidateQueries({ queryKey: ["feeds"] });
  };
}

export function useCreateCategory() {
  const invalidate = useInvalidate();
  return useMutation({
    mutationFn: (name: string) => api.post<Category>("/categories", { name }),
    onSuccess: invalidate,
  });
}

export function useUpdateCategory() {
  const invalidate = useInvalidate();
  return useMutation({
    mutationFn: ({ id, ...body }: { id: number; name?: string; position?: number }) =>
      api.patch<{ ok: boolean }>(`/categories/${id}`, body),
    onSuccess: invalidate,
  });
}

export function useDeleteCategory() {
  const invalidate = useInvalidate();
  return useMutation({
    mutationFn: (id: number) => api.del<{ ok: boolean }>(`/categories/${id}`),
    onSuccess: invalidate,
  });
}
