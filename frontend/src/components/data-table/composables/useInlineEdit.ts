import { ref, type Ref } from 'vue'

export interface EditingCell {
  rowId: string
  field: string
}

export interface UseInlineEditOptions {
  onSave: (id: string, field: string, value: unknown) => Promise<void> | void
  onCancel?: (id: string, field: string) => void
}

export function useInlineEdit(options: UseInlineEditOptions) {
  const editingCell: Ref<EditingCell | null> = ref(null)

  function startEdit(rowId: string, field: string) {
    editingCell.value = { rowId, field }
  }

  async function saveEdit(rowId: string, field: string, value: unknown) {
    await options.onSave(rowId, field, value)
    editingCell.value = null
  }

  function cancelEdit() {
    if (editingCell.value && options.onCancel) {
      options.onCancel(editingCell.value.rowId, editingCell.value.field)
    }
    editingCell.value = null
  }

  function isEditing(rowId: string, field: string): boolean {
    const cell = editingCell.value
    return cell !== null && cell.rowId === rowId && cell.field === field
  }

  return { editingCell, startEdit, saveEdit, cancelEdit, isEditing }
}
