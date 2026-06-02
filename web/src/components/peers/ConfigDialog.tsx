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
import { downloadText } from '@/lib/download';

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

type ConfigDialogProps = {
  open: boolean;
  config: string;
  filename: string;
  onClose: () => void;
};

export function ConfigDialog({ open, config, filename, onClose }: ConfigDialogProps) {
  const styles = useStyles();

  return (
    <Dialog open={open} onOpenChange={(_, data) => !data.open && onClose()}>
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
            <Button appearance="primary" onClick={() => downloadText(filename, config)}>
              Download .conf
            </Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
