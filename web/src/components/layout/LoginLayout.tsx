import { ShieldLockRegular } from '@fluentui/react-icons';
import type { ReactNode } from 'react';
import { useLoginPageStyles } from '@/styles/loginPage';

const DEFAULT_HERO_TITLE = 'Your private network hub.';
const DEFAULT_HERO_SUBTITLE =
  'Centralized WireGuard management for peers, groups, and secure access.';

type LoginLayoutProps = {
  children: ReactNode;
  wide?: boolean;
  scroll?: boolean;
  heroTitle?: string;
  heroSubtitle?: string;
};

function BrandMark({ hero = false }: { hero?: boolean }) {
  const styles = useLoginPageStyles();
  return (
    <span className={hero ? `${styles.logoMark} ${styles.logoMarkHero}` : styles.logoMark}>
      <ShieldLockRegular fontSize={hero ? 28 : 24} />
    </span>
  );
}

export function LoginLayout({
  children,
  wide = false,
  scroll = false,
  heroTitle = DEFAULT_HERO_TITLE,
  heroSubtitle = DEFAULT_HERO_SUBTITLE,
}: LoginLayoutProps) {
  const styles = useLoginPageStyles();
  const year = new Date().getFullYear();

  const formPanelClass = [
    styles.formPanel,
    scroll ? styles.formPanelScroll : '',
  ].filter(Boolean).join(' ');

  const formCardClass = [
    styles.formCard,
    wide ? styles.formCardWide : '',
    'login-animate-slide-up',
  ].filter(Boolean).join(' ');

  return (
    <div className={styles.shell}>
      <aside className={`${styles.hero} login-animate-fade-in`}>
        <div className={styles.heroInner}>
          <div className={styles.heroBrandRow}>
            <BrandMark hero />
            <span className={styles.heroBrandName}>WireHub</span>
          </div>
          <div>
            <h1 className={styles.heroTitle}>{heroTitle}</h1>
            <p className={styles.heroSubtitle}>{heroSubtitle}</p>
          </div>
        </div>
        <p className={styles.heroFooter}>© {year} WireHub. All rights reserved.</p>
      </aside>

      <main className={formPanelClass}>
        <div className={styles.mobileBrand}>
          <BrandMark />
          <span className={styles.mobileBrandName}>WireHub</span>
        </div>
        <div className={formCardClass}>
          {children}
        </div>
      </main>
    </div>
  );
}
