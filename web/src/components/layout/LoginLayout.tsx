import { ShieldLockRegular } from '@fluentui/react-icons';
import type { ReactNode } from 'react';
import { useLoginPageStyles } from '@/styles/loginPage';

type LoginLayoutProps = {
  children: ReactNode;
};

function BrandMark({ hero = false }: { hero?: boolean }) {
  const styles = useLoginPageStyles();
  return (
    <span className={hero ? `${styles.logoMark} ${styles.logoMarkHero}` : styles.logoMark}>
      <ShieldLockRegular fontSize={hero ? 28 : 24} />
    </span>
  );
}

export function LoginLayout({ children }: LoginLayoutProps) {
  const styles = useLoginPageStyles();
  const year = new Date().getFullYear();

  return (
    <div className={styles.shell}>
      <aside className={`${styles.hero} login-animate-fade-in`}>
        <div className={styles.heroInner}>
          <div className={styles.heroBrandRow}>
            <BrandMark hero />
            <span className={styles.heroBrandName}>WireHub</span>
          </div>
          <div>
            <h1 className={styles.heroTitle}>Your private network hub.</h1>
            <p className={styles.heroSubtitle}>
              Centralized WireGuard management for peers, groups, and secure access.
            </p>
          </div>
        </div>
        <p className={styles.heroFooter}>© {year} WireHub. All rights reserved.</p>
      </aside>

      <main className={styles.formPanel}>
        <div className={styles.mobileBrand}>
          <BrandMark />
          <span className={styles.mobileBrandName}>WireHub</span>
        </div>
        <div className={`${styles.formCard} login-animate-slide-up`}>
          {children}
        </div>
      </main>
    </div>
  );
}
