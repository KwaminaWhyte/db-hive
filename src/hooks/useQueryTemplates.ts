import { useState, useEffect, useCallback } from "react";
import { QueryTemplate } from "../types/templates";

const STORAGE_KEY = "dbhive_query_templates";

/**
 * Hook for managing query templates with localStorage persistence
 */
export function useQueryTemplates() {
  const [templates, setTemplates] = useState<QueryTemplate[]>([]);
  const [loading, setLoading] = useState(true);

  // Load templates from localStorage on mount
  useEffect(() => {
    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      if (stored) {
        const parsed = JSON.parse(stored);
        setTemplates(parsed);
      }
    } catch (error) {
      console.error("Failed to load templates:", error);
    } finally {
      setLoading(false);
    }
  }, []);

  // Save templates to localStorage whenever they change
  const saveToStorage = useCallback((newTemplates: QueryTemplate[]) => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(newTemplates));
    } catch (error) {
      console.error("Failed to save templates:", error);
    }
  }, []);

  const addTemplate = useCallback(
    (template: Omit<QueryTemplate, "id" | "createdAt" | "updatedAt">) => {
      const now = new Date().toISOString();
      const newTemplate: QueryTemplate = {
        ...template,
        id: crypto.randomUUID(),
        createdAt: now,
        updatedAt: now,
      };

      setTemplates((prev) => {
        const newTemplates = [...prev, newTemplate];
        saveToStorage(newTemplates);
        return newTemplates;
      });

      return newTemplate;
    },
    [saveToStorage]
  );

  const updateTemplate = useCallback(
    (id: string, updates: Partial<Omit<QueryTemplate, "id" | "createdAt">>) => {
      setTemplates((prev) => {
        const newTemplates = prev.map((t) =>
          t.id === id
            ? { ...t, ...updates, updatedAt: new Date().toISOString() }
            : t
        );
        saveToStorage(newTemplates);
        return newTemplates;
      });
    },
    [saveToStorage]
  );

  const deleteTemplate = useCallback(
    (id: string) => {
      setTemplates((prev) => {
        const newTemplates = prev.filter((t) => t.id !== id);
        saveToStorage(newTemplates);
        return newTemplates;
      });
    },
    [saveToStorage]
  );

  const getTemplate = useCallback(
    (id: string) => {
      return templates.find((t) => t.id === id);
    },
    [templates]
  );

  const getTemplatesByCategory = useCallback(
    (category?: string) => {
      if (!category) {
        return templates;
      }
      return templates.filter((t) => t.category === category);
    },
    [templates]
  );

  const getCategories = useCallback(() => {
    const categories = new Set<string>();
    templates.forEach((t) => {
      if (t.category) {
        categories.add(t.category);
      }
    });
    return Array.from(categories).sort();
  }, [templates]);

  const searchTemplates = useCallback(
    (query: string) => {
      const lower = query.toLowerCase();
      return templates.filter(
        (t) =>
          t.name.toLowerCase().includes(lower) ||
          t.description?.toLowerCase().includes(lower) ||
          t.sql.toLowerCase().includes(lower) ||
          t.tags?.some((tag) => tag.toLowerCase().includes(lower))
      );
    },
    [templates]
  );

  const duplicateTemplate = useCallback(
    (id: string) => {
      const original = templates.find((t) => t.id === id);
      if (!original) return null;

      return addTemplate({
        name: `${original.name} (Copy)`,
        description: original.description,
        sql: original.sql,
        parameters: [...original.parameters],
        category: original.category,
        tags: original.tags ? [...original.tags] : undefined,
      });
    },
    [templates, addTemplate]
  );

  const exportTemplates = useCallback(() => {
    const json = JSON.stringify(templates, null, 2);
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = `query-templates-${new Date().toISOString().split("T")[0]}.json`;
    link.click();
    URL.revokeObjectURL(url);
  }, [templates]);

  const importTemplates = useCallback(
    async (file: File) => {
      try {
        const text = await file.text();
        const imported = JSON.parse(text) as QueryTemplate[];

        // Validate structure
        if (!Array.isArray(imported)) {
          throw new Error("Invalid file format: expected an array");
        }

        // Merge with existing templates, avoiding duplicates by name
        const existingNames = new Set(templates.map((t) => t.name));
        const newTemplates: QueryTemplate[] = [];

        imported.forEach((t) => {
          if (!existingNames.has(t.name)) {
            newTemplates.push({
              ...t,
              id: crypto.randomUUID(),
              createdAt: t.createdAt || new Date().toISOString(),
              updatedAt: new Date().toISOString(),
            });
            existingNames.add(t.name);
          }
        });

        if (newTemplates.length > 0) {
          setTemplates((prev) => {
            const merged = [...prev, ...newTemplates];
            saveToStorage(merged);
            return merged;
          });
        }

        return newTemplates.length;
      } catch (error) {
        console.error("Failed to import templates:", error);
        throw error;
      }
    },
    [templates, saveToStorage]
  );

  return {
    templates,
    loading,
    addTemplate,
    updateTemplate,
    deleteTemplate,
    getTemplate,
    getTemplatesByCategory,
    getCategories,
    searchTemplates,
    duplicateTemplate,
    exportTemplates,
    importTemplates,
  };
}
