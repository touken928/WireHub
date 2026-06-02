import { Card } from '@fluentui/react-components';
import type { ReactNode } from 'react';
import { useAuthLayoutStyles } from '@/styles/authLayout';

type AuthLayoutProps = {
  children: ReactNode;
};

export function AuthLayout({ children }: AuthLayoutProps) {
  const styles = useAuthLayoutStyles();
  return (
    <div className={styles.page}>
      <Card className={styles.card}>{children}</Card>
    </div>
  );
}
