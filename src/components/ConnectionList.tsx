import { FC, useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  ConnectionProfile,
  getDriverDisplayName,
} from '../types/database';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';

interface ConnectionListProps {
  /** Callback when a profile is selected for editing */
  onEdit?: (profile: ConnectionProfile) => void;
  /** Callback when profiles list changes */
  onProfilesChange?: () => void;
  /** Callback when successfully connected to a database */
  onConnected?: (connectionId: string) => void;
}

export const ConnectionList: FC<ConnectionListProps> = ({
  onEdit,
  onProfilesChange,
  onConnected,
}) => {
  const [profiles, setProfiles] = useState<ConnectionProfile[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [connectingId, setConnectingId] = useState<string | null>(null);
  const [passwordPrompt, setPasswordPrompt] = useState<{
    profileId: string;
    profileName: string;
  } | null>(null);
  const [deletePrompt, setDeletePrompt] = useState<{
    profileId: string;
    profileName: string;
  } | null>(null);
  const [password, setPassword] = useState('');

  // Load profiles on mount
  useEffect(() => {
    loadProfiles();
  }, []);

  // Load connection profiles
  const loadProfiles = async () => {
    setLoading(true);
    setError(null);

    try {
      const result = await invoke<ConnectionProfile[]>(
        'list_connection_profiles'
      );
      setProfiles(result);
    } catch (err) {
      // Handle error - could be string or DbError object
      const errorMessage = typeof err === 'string'
        ? err
        : (err as any)?.message || String(err);
      setError(`Failed to load profiles: ${errorMessage}`);
    } finally {
      setLoading(false);
    }
  };

  // Handle connect button click
  const handleConnectClick = (profile: ConnectionProfile) => {
    setPasswordPrompt({
      profileId: profile.id,
      profileName: profile.name,
    });
    setPassword('');
  };

  // Handle connect with password
  const handleConnect = async () => {
    if (!passwordPrompt) return;

    setConnectingId(passwordPrompt.profileId);
    setError(null);

    try {
      const connectionId = await invoke<string>('connect_to_database', {
        profileId: passwordPrompt.profileId,
        password,
      });

      // Success - close password prompt
      setPasswordPrompt(null);
      setPassword('');

      // Notify parent component
      onConnected?.(connectionId);
    } catch (err) {
      // Handle error - could be string or DbError object
      const errorMessage = typeof err === 'string'
        ? err
        : (err as any)?.message || String(err);
      setError(`Failed to connect: ${errorMessage}`);
    } finally {
      setConnectingId(null);
    }
  };

  // Handle delete button click
  const handleDeleteClick = (profile: ConnectionProfile) => {
    setDeletePrompt({
      profileId: profile.id,
      profileName: profile.name,
    });
  };

  // Handle delete confirmation
  const handleDeleteConfirm = async () => {
    if (!deletePrompt) return;

    setError(null);

    try {
      await invoke('delete_connection_profile', {
        profileId: deletePrompt.profileId,
      });
      await loadProfiles();
      onProfilesChange?.();
      setDeletePrompt(null);
    } catch (err) {
      // Handle error - could be string or DbError object
      const errorMessage = typeof err === 'string'
        ? err
        : (err as any)?.message || String(err);
      setError(`Failed to delete profile: ${errorMessage}`);
    }
  };

  if (loading) {
    return (
      <Card className="m-6">
        <CardHeader>
          <CardTitle>Connections</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-muted-foreground">Loading...</p>
        </CardContent>
      </Card>
    );
  }

  return (
    <div className="p-6 space-y-4">
      <div className="flex justify-between items-center">
        <div>
          <h2 className="text-2xl font-bold">Connections</h2>
          <p className="text-sm text-muted-foreground">
            Manage your database connections
          </p>
        </div>
        <Button variant="outline" onClick={loadProfiles}>
          Refresh
        </Button>
      </div>

      {error && (
        <div className="rounded-md bg-destructive/10 p-3 text-sm text-destructive border border-destructive/20">
          {error}
        </div>
      )}

      {profiles.length === 0 ? (
        <Card>
          <CardContent className="pt-6">
            <p className="text-muted-foreground text-center">
              No connections yet. Create one using the form on the right.
            </p>
          </CardContent>
        </Card>
      ) : (
        <div className="space-y-3">
          {profiles.map((profile) => (
            <Card
              key={profile.id}
              className="hover:shadow-md transition-shadow"
            >
              <CardHeader className="pb-3">
                <CardTitle className="text-lg">{profile.name}</CardTitle>
                <CardDescription className="space-y-1 text-xs">
                  <div>
                    <span className="font-medium">Driver:</span>{' '}
                    {getDriverDisplayName(profile.driver)}
                  </div>
                  <div>
                    <span className="font-medium">Host:</span> {profile.host}:
                    {profile.port}
                  </div>
                  <div>
                    <span className="font-medium">Username:</span>{' '}
                    {profile.username}
                  </div>
                  {profile.database && (
                    <div>
                      <span className="font-medium">Database:</span>{' '}
                      {profile.database}
                    </div>
                  )}
                </CardDescription>
              </CardHeader>
              <CardContent>
                <div className="flex gap-2">
                  <Button
                    onClick={() => handleConnectClick(profile)}
                    disabled={connectingId !== null}
                    className="flex-1"
                  >
                    {connectingId === profile.id ? 'Connecting...' : 'Connect'}
                  </Button>

                  <Button
                    variant="ghost"
                    onClick={() => onEdit?.(profile)}
                    className="flex-1"
                  >
                    Edit
                  </Button>

                  <Button
                    variant="destructive"
                    onClick={() => handleDeleteClick(profile)}
                    className="flex-1"
                  >
                    Delete
                  </Button>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      {/* Password Prompt Dialog */}
      <Dialog
        open={!!passwordPrompt}
        onOpenChange={(open) => !open && setPasswordPrompt(null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Connect to {passwordPrompt?.profileName}</DialogTitle>
            <DialogDescription>
              Enter the password to connect to this database.
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-2 py-4">
            <Label htmlFor="connect-password">Password</Label>
            <Input
              type="password"
              id="connect-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  handleConnect();
                }
              }}
              placeholder="Enter password"
              autoFocus
            />
          </div>

          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setPasswordPrompt(null)}
            >
              Cancel
            </Button>
            <Button
              onClick={handleConnect}
              disabled={!password || connectingId !== null}
            >
              {connectingId ? 'Connecting...' : 'Connect'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete Confirmation Dialog */}
      <Dialog
        open={!!deletePrompt}
        onOpenChange={(open) => !open && setDeletePrompt(null)}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Connection</DialogTitle>
            <DialogDescription>
              Are you sure you want to delete "{deletePrompt?.profileName}"?
              This action cannot be undone.
            </DialogDescription>
          </DialogHeader>

          <DialogFooter>
            <Button variant="outline" onClick={() => setDeletePrompt(null)}>
              Cancel
            </Button>
            <Button variant="destructive" onClick={handleDeleteConfirm}>
              Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
};
