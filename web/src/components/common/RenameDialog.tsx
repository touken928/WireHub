import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Field,
  Input,
  Text,
  tokens,
} from '@fluentui/react-components';

type RenameDialogProps = {
  open: boolean;
  title: string;
  label: string;
  value: string;
  error?: string;
  onValueChange: (value: string) => void;
  onClose: () => void;
  onSave: () => void;
};

export function RenameDialog({
  open,
  title,
  label,
  value,
  error,
  onValueChange,
  onClose,
  onSave,
}: RenameDialogProps) {
  return (
    <Dialog open={open} onOpenChange={(_, data) => !data.open && onClose()}>
      <DialogSurface>
        <DialogBody>
          <DialogTitle>{title}</DialogTitle>
          <DialogContent>
            <Field label={label} required>
              <Input value={value} onChange={(_, data) => onValueChange(data.value)} />
            </Field>
            {error && (
              <Text size={200} style={{ color: tokens.colorPaletteRedForeground1 }}>{error}</Text>
            )}
          </DialogContent>
          <DialogActions>
            <Button onClick={onClose}>Cancel</Button>
            <Button appearance="primary" onClick={onSave} disabled={!value.trim()}>
              Save
            </Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
