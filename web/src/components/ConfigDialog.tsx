import {
  Dialog,
  DialogSurface,
  DialogBody,
  DialogTitle,
  DialogContent,
  DialogActions,
  Button,
  Text,
  makeStyles,
  tokens,
} from '@fluentui/react-components';
import { QRCodeSVG } from 'qrcode.react';

const useStyles = makeStyles({
  content: {
    display: 'grid',
    gridTemplateColumns: '220px minmax(0, 1fr)',
    gap: '20px',
    alignItems: 'stretch',
    '@media (max-width: 640px)': {
      gridTemplateColumns: '1fr',
    },
  },
  qrWrap: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    padding: '16px',
    borderRadius: tokens.borderRadiusXLarge,
    backgroundColor: tokens.colorNeutralBackground2,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
  },
  configBox: {
    fontFamily: tokens.fontFamilyMonospace,
    fontSize: '12px',
    whiteSpace: 'pre-wrap',
    maxHeight: '260px',
    overflow: 'auto',
    width: '100%',
    boxSizing: 'border-box',
    backgroundColor: tokens.colorNeutralBackground2,
    border: `1px solid ${tokens.colorNeutralStroke2}`,
    padding: '12px',
    borderRadius: tokens.borderRadiusLarge,
  },
});

interface ConfigDialogProps {
  open: boolean;
  config: string;
  filename: string;
  onClose: () => void;
}

export default function ConfigDialog({ open, config, filename, onClose }: ConfigDialogProps) {
  const styles = useStyles();

  const download = () => {
    const blob = new Blob([config], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <Dialog open={open} onOpenChange={(_, d) => !d.open && onClose()}>
      <DialogSurface>
        <DialogBody>
          <DialogTitle>Client Configuration</DialogTitle>
          <DialogContent className={styles.content}>
            <div className={styles.qrWrap}>
              <QRCodeSVG value={config} size={188} />
            </div>
            <Text className={styles.configBox}>{config}</Text>
          </DialogContent>
          <DialogActions>
            <Button appearance="secondary" onClick={onClose}>Close</Button>
            <Button appearance="primary" onClick={download}>Download .conf</Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
