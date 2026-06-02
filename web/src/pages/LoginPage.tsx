import {
  Card,
  Input,
  Button,
  Title1,
  Text,
  makeStyles,
  tokens,
} from '@fluentui/react-components';
import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, setToken } from '../api/client';

const useStyles = makeStyles({
  page: {
    minHeight: '100vh',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    background: `linear-gradient(135deg, ${tokens.colorBrandBackground2} 0%, ${tokens.colorNeutralBackground2} 100%)`,
  },
  card: {
    width: '400px',
    padding: '32px',
    display: 'flex',
    flexDirection: 'column',
    gap: '16px',
  },
  error: {
    color: tokens.colorPaletteRedForeground1,
  },
});

export default function LoginPage() {
  const styles = useStyles();
  const navigate = useNavigate();
  const [username, setUsername] = useState('admin');
  const [password, setPassword] = useState('admin');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoading(true);
    setError('');
    try {
      const { token } = await api.login(username, password);
      setToken(token);
      navigate('/');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Login failed');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className={styles.page}>
      <Card className={styles.card}>
        <Title1>WireHub</Title1>
        <Text>Centralized WireGuard Hub Manager</Text>
        <form onSubmit={handleSubmit} style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <Input
            placeholder="Username"
            value={username}
            onChange={(_, d) => setUsername(d.value)}
          />
          <Input
            type="password"
            placeholder="Password"
            value={password}
            onChange={(_, d) => setPassword(d.value)}
          />
          {error && <Text className={styles.error}>{error}</Text>}
          <Button appearance="primary" type="submit" disabled={loading}>
            {loading ? 'Signing in...' : 'Sign in'}
          </Button>
        </form>
      </Card>
    </div>
  );
}
